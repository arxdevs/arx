//! Webhook delivery transports.
//!
//! The extensibility seam is a transport trait, not a payload formatter: the
//! real per-channel variation is transport + auth + success/retry classification
//! (a generic webhook signs with HMAC over HTTP; Slack/Discord use a secret URL;
//! email would be SMTP). Only the `webhook` kind is implemented now.

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Transport-neutral delivery result. Retry/dead-letter logic keys off this, not
/// off an HTTP status, so future non-HTTP transports (e.g. SMTP) fit unchanged.
#[derive(Debug)]
pub enum DeliveryOutcome {
    Delivered {
        response_status: Option<i64>,
        response_size: Option<i64>,
    },
    /// Transient failure; should be retried with backoff.
    Retryable {
        response_status: Option<i64>,
        reason: String,
    },
    /// Permanent failure; should be dead-lettered without further retries.
    Permanent {
        response_status: Option<i64>,
        reason: String,
    },
}

/// Computes the `X-Arx-Signature-256` value over `"<timestamp>.<body>"`.
///
/// The timestamp is part of the signed input (Stripe-style) so receivers can
/// reject stale deliveries within a tolerance window. Mirrors the hex format
/// that `arx_github::verify_signature` consumes.
pub fn sign(secret: &[u8], timestamp: i64, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let bytes = mac.finalize().into_bytes();
    format!("sha256={}", hex::encode(bytes))
}

/// A transport knows how to deliver a serialized event to one endpoint.
#[async_trait::async_trait]
pub trait WebhookTransport: Send + Sync {
    async fn deliver(
        &self,
        http: &reqwest::Client,
        url: &str,
        credentials: &serde_json::Value,
        delivery_id: &str,
        event_type: &str,
        body: &str,
    ) -> DeliveryOutcome;
}

/// The generic signed-JSON webhook transport (kind = `webhook`).
pub struct GenericWebhook;

#[async_trait::async_trait]
impl WebhookTransport for GenericWebhook {
    async fn deliver(
        &self,
        http: &reqwest::Client,
        url: &str,
        credentials: &serde_json::Value,
        delivery_id: &str,
        event_type: &str,
        body: &str,
    ) -> DeliveryOutcome {
        let secret = credentials
            .get("signing_secret")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let ts = chrono::Utc::now().timestamp();
        let signature = sign(secret.as_bytes(), ts, body.as_bytes());

        let resp = http
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-Arx-Event", event_type)
            .header("X-Arx-Delivery", delivery_id)
            .header("X-Arx-Timestamp", ts.to_string())
            .header("X-Arx-Signature-256", signature)
            .body(body.to_string())
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                let code = Some(status.as_u16() as i64);
                // Read body only to measure size; never store the body (avoids an
                // SSRF read-oracle). Bounded by reqwest's response handling.
                let size = r.bytes().await.map(|b| b.len() as i64).ok();
                if status.is_success() {
                    DeliveryOutcome::Delivered {
                        response_status: code,
                        response_size: size,
                    }
                } else if status.is_client_error() {
                    // 4xx (incl. redirects, since redirects are disabled and 3xx
                    // surfaces as a non-success non-5xx) => permanent.
                    DeliveryOutcome::Permanent {
                        response_status: code,
                        reason: format!("http {}", status.as_u16()),
                    }
                } else {
                    // 3xx (redirects disabled) and 5xx => retryable.
                    DeliveryOutcome::Retryable {
                        response_status: code,
                        reason: format!("http {}", status.as_u16()),
                    }
                }
            }
            Err(e) => {
                // Network/timeout/blocked-address (SSRF guard) => retryable.
                // The message is a coarse class, never the secret or payload.
                let reason = if e.is_timeout() {
                    "timeout".to_string()
                } else if e.is_connect() {
                    "connect_error".to_string()
                } else {
                    "request_error".to_string()
                };
                DeliveryOutcome::Retryable {
                    response_status: None,
                    reason,
                }
            }
        }
    }
}

/// Selects the transport for an endpoint kind.
pub fn transport_for(kind: &str) -> Option<Box<dyn WebhookTransport>> {
    match kind {
        "webhook" => Some(Box::new(GenericWebhook)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_roundtrips_with_github_verify() {
        // The receiver reconstructs "<ts>.<body>" and verifies with the same hex
        // HMAC-SHA256 scheme arx_github::verify_signature uses.
        let secret = b"topsecret";
        let ts = 1_700_000_000i64;
        let body = br#"{"type":"deployment.succeeded"}"#;
        let header = sign(secret, ts, body);

        let mut signed_input = Vec::new();
        signed_input.extend_from_slice(ts.to_string().as_bytes());
        signed_input.push(b'.');
        signed_input.extend_from_slice(body);

        assert!(
            arx_github::verify_signature(secret, Some(&header), &signed_input).is_ok(),
            "generated signature must verify against arx_github::verify_signature"
        );
    }

    #[test]
    fn sign_detects_tamper() {
        let secret = b"topsecret";
        let ts = 1_700_000_000i64;
        let header = sign(secret, ts, b"original");

        let mut tampered = Vec::new();
        tampered.extend_from_slice(ts.to_string().as_bytes());
        tampered.push(b'.');
        tampered.extend_from_slice(b"tampered");

        assert!(arx_github::verify_signature(secret, Some(&header), &tampered).is_err());
    }
}
