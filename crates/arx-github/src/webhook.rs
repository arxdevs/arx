use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WebhookError {
    #[error("missing X-Hub-Signature-256 header")]
    MissingHeader,
    #[error("bad signature format")]
    BadFormat,
    #[error("signature mismatch")]
    Mismatch,
}

pub fn verify_signature(
    secret: &[u8],
    signature_header: Option<&str>,
    payload: &[u8],
) -> Result<(), WebhookError> {
    let header = signature_header.ok_or(WebhookError::MissingHeader)?;
    let expected_hex = header
        .strip_prefix("sha256=")
        .ok_or(WebhookError::BadFormat)?;
    let expected = hex::decode(expected_hex).map_err(|_| WebhookError::BadFormat)?;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret).map_err(|_| WebhookError::BadFormat)?;
    mac.update(payload);
    let computed = mac.finalize().into_bytes();

    if computed.as_slice().ct_eq(&expected).into() {
        Ok(())
    } else {
        Err(WebhookError::Mismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &[u8], payload: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(payload);
        let bytes = mac.finalize().into_bytes();
        format!("sha256={}", hex::encode(bytes))
    }

    #[test]
    fn valid_signature_accepts() {
        let secret = b"shhh";
        let payload = b"{\"hello\":\"world\"}";
        let sig = sign(secret, payload);
        assert!(verify_signature(secret, Some(&sig), payload).is_ok());
    }

    #[test]
    fn tampered_payload_rejects() {
        let secret = b"shhh";
        let payload = b"{\"hello\":\"world\"}";
        let sig = sign(secret, payload);
        let tampered = b"{\"hello\":\"WORLD\"}";
        assert!(matches!(
            verify_signature(secret, Some(&sig), tampered),
            Err(WebhookError::Mismatch)
        ));
    }

    #[test]
    fn missing_header_rejects() {
        assert!(matches!(
            verify_signature(b"x", None, b""),
            Err(WebhookError::MissingHeader)
        ));
    }

    #[test]
    fn bad_format_rejects() {
        assert!(matches!(
            verify_signature(b"x", Some("md5=zzz"), b""),
            Err(WebhookError::BadFormat)
        ));
        assert!(matches!(
            verify_signature(b"x", Some("sha256=notvalidhex"), b""),
            Err(WebhookError::BadFormat)
        ));
    }
}
