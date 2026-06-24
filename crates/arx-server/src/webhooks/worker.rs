//! Delivery worker and pruner for outgoing webhooks.

use super::transport::{self, DeliveryOutcome};
use crate::state::AppState;
use crate::supervisor::spawn_supervised;
use arx_core::model::WebhookDelivery;
use chrono::{Duration as ChronoDuration, Utc};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{Duration, MissedTickBehavior, interval};
use tracing::{debug, warn};

/// Worker tick. Short so retries scheduled "immediately/30s" fire promptly.
const TICK: Duration = Duration::from_secs(5);
/// How many deliveries to claim per tick.
const CLAIM_BATCH: i64 = 32;
/// Lease for an in-flight claim; reclaimed if the worker dies mid-send.
const LEASE_SECONDS: i64 = 60;
/// Cap on concurrent in-flight deliveries. Also bounds the worker's share of the
/// shared SQLite connection pool so a delivery burst can't starve the deploy
/// path (gates both the HTTP send and the surrounding DB writes).
const MAX_CONCURRENCY: usize = 2;

/// Backoff schedule (seconds from now) keyed by the attempt number that just
/// failed (1-based). After the last entry, the delivery is dead-lettered.
const BACKOFF_SECONDS: &[i64] = &[30, 5 * 60, 30 * 60, 2 * 60 * 60, 6 * 60 * 60];

/// Auto-disable thresholds: only disable an endpoint after sustained failure,
/// not a brief outage. Requires both a minimum failure count AND a minimum
/// elapsed window since the first failure of the streak.
const AUTO_DISABLE_MIN_FAILURES: i64 = 20;
const AUTO_DISABLE_MIN_WINDOW: ChronoDuration = ChronoDuration::hours(12);

/// Delivery retention. Non-dead-letter rows older than this are pruned;
/// dead-letters (exhausted) are kept so users can inspect/redeliver them.
const RETAIN_DAYS: i64 = 30;
const PRUNE_TICK: Duration = Duration::from_secs(60 * 60 * 24);

pub fn spawn_worker(app: AppState) {
    let client = super::build_delivery_client();
    spawn_supervised("outgoing_webhooks", move || {
        let app = app.clone();
        let client = client.clone();
        async move {
            // Recover any deliveries left in_flight by a previous crash.
            if let Err(e) = arx_db::queries::webhooks::reclaim_expired(&app.db, Utc::now()).await {
                warn!(error = %e, "webhook worker: reclaim_expired failed");
            }
            let sem = Arc::new(Semaphore::new(MAX_CONCURRENCY));
            let mut ticker = interval(TICK);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                // Reclaim leases that expired since last tick.
                let _ = arx_db::queries::webhooks::reclaim_expired(&app.db, Utc::now()).await;

                let due = match arx_db::queries::webhooks::claim_due(
                    &app.db,
                    Utc::now(),
                    LEASE_SECONDS,
                    CLAIM_BATCH,
                )
                .await
                {
                    Ok(d) => d,
                    Err(e) => {
                        warn!(error = %e, "webhook worker: claim_due failed");
                        continue;
                    }
                };
                if due.is_empty() {
                    continue;
                }

                let mut handles = Vec::new();
                for delivery in due {
                    let permit = match sem.clone().acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => break,
                    };
                    let app = app.clone();
                    let client = client.clone();
                    handles.push(tokio::spawn(async move {
                        deliver_one(&app, &client, delivery).await;
                        drop(permit);
                    }));
                }
                for h in handles {
                    let _ = h.await;
                }
            }
        }
    });
}

async fn deliver_one(app: &AppState, client: &reqwest::Client, delivery: WebhookDelivery) {
    let endpoint = match arx_db::queries::webhooks::get(&app.db, delivery.endpoint_id).await {
        Ok(ep) => ep,
        Err(e) => {
            warn!(error = %e, delivery = %delivery.id.as_uuid(), "webhook deliver: endpoint gone");
            // Endpoint vanished; dead-letter so it doesn't loop forever.
            let _ = arx_db::queries::webhooks::mark_exhausted(
                &app.db,
                delivery.id,
                None,
                "endpoint_missing",
            )
            .await;
            return;
        }
    };

    let transport = match transport::transport_for(&endpoint.kind) {
        Some(t) => t,
        None => {
            let _ = arx_db::queries::webhooks::mark_exhausted(
                &app.db,
                delivery.id,
                None,
                "unsupported_kind",
            )
            .await;
            return;
        }
    };

    let credentials =
        match arx_db::queries::webhooks::credentials_for(&app.db, &app.master_key, endpoint.id)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, endpoint = %endpoint.id.as_uuid(), "webhook deliver: credential decrypt failed");
                let _ = arx_db::queries::webhooks::mark_retryable(
                    &app.db,
                    delivery.id,
                    next_attempt_at(delivery.attempts),
                    None,
                    "credential_error",
                )
                .await;
                return;
            }
        };

    let outcome = transport
        .deliver(
            client,
            &endpoint.url,
            &credentials,
            &delivery.id.as_uuid().to_string(),
            &delivery.event_type,
            &delivery.payload,
        )
        .await;

    match outcome {
        DeliveryOutcome::Delivered {
            response_status,
            response_size,
        } => {
            let _ = arx_db::queries::webhooks::mark_success(
                &app.db,
                delivery.id,
                response_status,
                response_size,
            )
            .await;
            let _ = arx_db::queries::webhooks::record_success(&app.db, endpoint.id).await;
            debug!(endpoint = %endpoint.id.as_uuid(), "webhook delivered");
        }
        DeliveryOutcome::Permanent {
            response_status,
            reason,
        } => {
            let _ = arx_db::queries::webhooks::mark_exhausted(
                &app.db,
                delivery.id,
                response_status,
                &reason,
            )
            .await;
            record_failure_and_maybe_disable(app, endpoint.id).await;
        }
        DeliveryOutcome::Retryable {
            response_status,
            reason,
        } => {
            // attempts is the count *before* this attempt; the next failed
            // attempt number is attempts + 1.
            let attempt_just_failed = delivery.attempts + 1;
            if (attempt_just_failed as usize) >= BACKOFF_SECONDS.len() + 1 {
                // Exhausted the retry schedule.
                let _ = arx_db::queries::webhooks::mark_exhausted(
                    &app.db,
                    delivery.id,
                    response_status,
                    &reason,
                )
                .await;
            } else {
                let _ = arx_db::queries::webhooks::mark_retryable(
                    &app.db,
                    delivery.id,
                    next_attempt_at(delivery.attempts),
                    response_status,
                    &reason,
                )
                .await;
            }
            record_failure_and_maybe_disable(app, endpoint.id).await;
        }
    }
}

/// Next attempt time given the number of attempts already made (0-based before
/// the current one). Clamps to the last schedule entry.
fn next_attempt_at(attempts_before: i64) -> chrono::DateTime<Utc> {
    let idx = (attempts_before as usize).min(BACKOFF_SECONDS.len() - 1);
    Utc::now() + ChronoDuration::seconds(BACKOFF_SECONDS[idx])
}

async fn record_failure_and_maybe_disable(app: &AppState, endpoint_id: arx_core::ids::WebhookEndpointId) {
    match arx_db::queries::webhooks::record_failure(&app.db, endpoint_id).await {
        Ok((count, first_failure_at)) => {
            let window_ok = first_failure_at
                .map(|t| Utc::now() - t >= AUTO_DISABLE_MIN_WINDOW)
                .unwrap_or(false);
            if count >= AUTO_DISABLE_MIN_FAILURES && window_ok {
                let _ = arx_db::queries::webhooks::disable(
                    &app.db,
                    endpoint_id,
                    "auto-disabled after sustained delivery failures",
                )
                .await;
                let _ = arx_db::queries::audit::write(
                    &app.db,
                    None,
                    "webhook.auto_disabled",
                    &format!("webhook_endpoint:{}", endpoint_id.as_uuid()),
                    serde_json::json!({ "consecutive_failures": count }),
                )
                .await;
                warn!(endpoint = %endpoint_id.as_uuid(), "webhook endpoint auto-disabled");
            }
        }
        Err(e) => warn!(error = %e, "webhook deliver: record_failure failed"),
    }
}

pub fn spawn_pruner(app: AppState) {
    spawn_supervised("webhook_delivery_pruner", move || {
        let app = app.clone();
        async move {
            let mut ticker = interval(PRUNE_TICK);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let cutoff = Utc::now() - ChronoDuration::days(RETAIN_DAYS);
                match arx_db::queries::webhooks::prune_old(&app.db, cutoff).await {
                    Ok(n) if n > 0 => debug!(deleted = n, "webhook deliveries pruned"),
                    Ok(_) => {}
                    Err(e) => warn!(error = %e, "webhook delivery prune failed"),
                }
            }
        }
    });
}
