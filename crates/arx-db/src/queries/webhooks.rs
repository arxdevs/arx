use super::map_sqlx;
use crate::crypto::MasterKey;
use arx_core::ids::{ProjectId, WebhookDeliveryId, WebhookEndpointId, WorkspaceId};
use arx_core::model::{DeliveryStatus, WebhookDelivery, WebhookEndpoint};
use arx_core::{Error, Result};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

/// Creates an endpoint, encrypting the credential JSON blob at rest with the
/// same ChaCha20-Poly1305 scheme used for service variables.
#[allow(clippy::too_many_arguments)]
pub async fn create(
    pool: &SqlitePool,
    key: &MasterKey,
    workspace_id: WorkspaceId,
    project_id: Option<ProjectId>,
    kind: &str,
    url: &str,
    config: &serde_json::Value,
    credentials: &serde_json::Value,
    events: &[String],
) -> Result<WebhookEndpoint> {
    let id = WebhookEndpointId::new();
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let cred_bytes = serde_json::to_vec(credentials).map_err(|e| Error::Internal(e.to_string()))?;
    let (ct, nonce) = key.encrypt(&cred_bytes)?;
    let config_str = serde_json::to_string(config).map_err(|e| Error::Internal(e.to_string()))?;
    let events_str = serde_json::to_string(events).map_err(|e| Error::Internal(e.to_string()))?;

    sqlx::query(
        "INSERT INTO outgoing_webhook_endpoints
         (id, workspace_id, project_id, kind, url, config, secret_ct, secret_nonce,
          events, active, consecutive_failures, first_failure_at, disabled_reason,
          created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 0, NULL, NULL, ?, ?)",
    )
    .bind(id.as_uuid().to_string())
    .bind(workspace_id.as_uuid().to_string())
    .bind(project_id.map(|p| p.as_uuid().to_string()))
    .bind(kind)
    .bind(url)
    .bind(&config_str)
    .bind(&ct)
    .bind(&nonce)
    .bind(&events_str)
    .bind(&now_str)
    .bind(&now_str)
    .execute(pool)
    .await
    .map_err(map_sqlx)?;

    get(pool, id).await
}

pub async fn get(pool: &SqlitePool, id: WebhookEndpointId) -> Result<WebhookEndpoint> {
    let row = sqlx::query(ENDPOINT_COLS)
        .bind(id.as_uuid().to_string())
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx)?
        .ok_or(Error::NotFound)?;
    parse_endpoint(&row)
}

pub async fn list_in_workspace(
    pool: &SqlitePool,
    workspace_id: WorkspaceId,
) -> Result<Vec<WebhookEndpoint>> {
    let rows = sqlx::query(
        "SELECT id, workspace_id, project_id, kind, url, config, secret_ct, secret_nonce,
                events, active, consecutive_failures, first_failure_at, disabled_reason,
                created_at, updated_at
         FROM outgoing_webhook_endpoints
         WHERE workspace_id = ?
         ORDER BY created_at DESC",
    )
    .bind(workspace_id.as_uuid().to_string())
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;
    rows.iter().map(parse_endpoint).collect()
}

/// Returns active endpoints in a workspace that should receive a given event
/// type for a given project. Event-type membership is matched in Rust (not SQL
/// LIKE — substring matching would mis-fire, e.g. `deployment.fail` vs
/// `deployment.failed`). `["*"]` matches every type.
pub async fn list_active_for_event(
    pool: &SqlitePool,
    workspace_id: WorkspaceId,
    project_id: Option<ProjectId>,
    event_type: &str,
) -> Result<Vec<WebhookEndpoint>> {
    let rows = sqlx::query(
        "SELECT id, workspace_id, project_id, kind, url, config, secret_ct, secret_nonce,
                events, active, consecutive_failures, first_failure_at, disabled_reason,
                created_at, updated_at
         FROM outgoing_webhook_endpoints
         WHERE workspace_id = ? AND active = 1
           AND (project_id IS NULL OR project_id = ?)",
    )
    .bind(workspace_id.as_uuid().to_string())
    .bind(project_id.map(|p| p.as_uuid().to_string()))
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;

    let mut out = Vec::new();
    for row in &rows {
        let ep = parse_endpoint(row)?;
        if ep
            .events
            .iter()
            .any(|e| e == "*" || e == event_type)
        {
            out.push(ep);
        }
    }
    Ok(out)
}

/// Decrypts and returns the credential JSON for an endpoint (worker-only path).
pub async fn credentials_for(
    pool: &SqlitePool,
    key: &MasterKey,
    id: WebhookEndpointId,
) -> Result<serde_json::Value> {
    let ep = get(pool, id).await?;
    let bytes = key.decrypt(&ep.secret_ct, &ep.secret_nonce)?;
    serde_json::from_slice(&bytes).map_err(|e| Error::Internal(e.to_string()))
}

pub async fn update(
    pool: &SqlitePool,
    id: WebhookEndpointId,
    url: Option<&str>,
    events: Option<&[String]>,
    active: Option<bool>,
    project_id: Option<Option<ProjectId>>,
) -> Result<WebhookEndpoint> {
    let now = Utc::now().to_rfc3339();
    if let Some(u) = url {
        sqlx::query("UPDATE outgoing_webhook_endpoints SET url = ?, updated_at = ? WHERE id = ?")
            .bind(u)
            .bind(&now)
            .bind(id.as_uuid().to_string())
            .execute(pool)
            .await
            .map_err(map_sqlx)?;
    }
    if let Some(ev) = events {
        let ev_str = serde_json::to_string(ev).map_err(|e| Error::Internal(e.to_string()))?;
        sqlx::query("UPDATE outgoing_webhook_endpoints SET events = ?, updated_at = ? WHERE id = ?")
            .bind(&ev_str)
            .bind(&now)
            .bind(id.as_uuid().to_string())
            .execute(pool)
            .await
            .map_err(map_sqlx)?;
    }
    if let Some(a) = active {
        sqlx::query("UPDATE outgoing_webhook_endpoints SET active = ?, updated_at = ? WHERE id = ?")
            .bind(if a { 1 } else { 0 })
            .bind(&now)
            .bind(id.as_uuid().to_string())
            .execute(pool)
            .await
            .map_err(map_sqlx)?;
    }
    if let Some(pid) = project_id {
        sqlx::query(
            "UPDATE outgoing_webhook_endpoints SET project_id = ?, updated_at = ? WHERE id = ?",
        )
        .bind(pid.map(|p| p.as_uuid().to_string()))
        .bind(&now)
        .bind(id.as_uuid().to_string())
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    }
    get(pool, id).await
}

pub async fn delete(pool: &SqlitePool, id: WebhookEndpointId) -> Result<()> {
    let res = sqlx::query("DELETE FROM outgoing_webhook_endpoints WHERE id = ?")
        .bind(id.as_uuid().to_string())
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    if res.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    Ok(())
}

/// Records a successful delivery: resets the failure window so a transient
/// outage does not accumulate toward auto-disable.
pub async fn record_success(pool: &SqlitePool, id: WebhookEndpointId) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE outgoing_webhook_endpoints
         SET consecutive_failures = 0, first_failure_at = NULL, updated_at = ?
         WHERE id = ?",
    )
    .bind(&now)
    .bind(id.as_uuid().to_string())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

/// Records a failed delivery and returns the resulting (consecutive_failures,
/// first_failure_at) so the worker can decide whether to auto-disable.
pub async fn record_failure(
    pool: &SqlitePool,
    id: WebhookEndpointId,
) -> Result<(i64, Option<DateTime<Utc>>)> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    // first_failure_at is set only on the first failure of a streak.
    sqlx::query(
        "UPDATE outgoing_webhook_endpoints
         SET consecutive_failures = consecutive_failures + 1,
             first_failure_at = COALESCE(first_failure_at, ?),
             updated_at = ?
         WHERE id = ?",
    )
    .bind(&now_str)
    .bind(&now_str)
    .bind(id.as_uuid().to_string())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    let ep = get(pool, id).await?;
    Ok((ep.consecutive_failures, ep.first_failure_at))
}

pub async fn disable(pool: &SqlitePool, id: WebhookEndpointId, reason: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE outgoing_webhook_endpoints
         SET active = 0, disabled_reason = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(reason)
    .bind(&now)
    .bind(id.as_uuid().to_string())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

pub async fn enable(pool: &SqlitePool, id: WebhookEndpointId) -> Result<WebhookEndpoint> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE outgoing_webhook_endpoints
         SET active = 1, disabled_reason = NULL, consecutive_failures = 0,
             first_failure_at = NULL, updated_at = ?
         WHERE id = ?",
    )
    .bind(&now)
    .bind(id.as_uuid().to_string())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    get(pool, id).await
}

const ENDPOINT_COLS: &str =
    "SELECT id, workspace_id, project_id, kind, url, config, secret_ct, secret_nonce,
            events, active, consecutive_failures, first_failure_at, disabled_reason,
            created_at, updated_at
     FROM outgoing_webhook_endpoints WHERE id = ?";

fn parse_endpoint(row: &SqliteRow) -> Result<WebhookEndpoint> {
    let id: String = row.try_get("id").map_err(map_sqlx)?;
    let workspace_id: String = row.try_get("workspace_id").map_err(map_sqlx)?;
    let project_id: Option<String> = row.try_get("project_id").map_err(map_sqlx)?;
    let config_str: String = row.try_get("config").map_err(map_sqlx)?;
    let events_str: String = row.try_get("events").map_err(map_sqlx)?;
    let active: i64 = row.try_get("active").map_err(map_sqlx)?;
    let first_failure_at: Option<String> = row.try_get("first_failure_at").map_err(map_sqlx)?;
    let created_at: String = row.try_get("created_at").map_err(map_sqlx)?;
    let updated_at: String = row.try_get("updated_at").map_err(map_sqlx)?;

    Ok(WebhookEndpoint {
        id: WebhookEndpointId::from_uuid(parse_uuid(&id)?),
        workspace_id: WorkspaceId::from_uuid(parse_uuid(&workspace_id)?),
        project_id: match project_id {
            Some(p) => Some(ProjectId::from_uuid(parse_uuid(&p)?)),
            None => None,
        },
        kind: row.try_get("kind").map_err(map_sqlx)?,
        url: row.try_get("url").map_err(map_sqlx)?,
        config: serde_json::from_str(&config_str).map_err(|e| Error::Internal(e.to_string()))?,
        secret_ct: row.try_get("secret_ct").map_err(map_sqlx)?,
        secret_nonce: row.try_get("secret_nonce").map_err(map_sqlx)?,
        events: serde_json::from_str(&events_str).map_err(|e| Error::Internal(e.to_string()))?,
        active: active != 0,
        consecutive_failures: row.try_get("consecutive_failures").map_err(map_sqlx)?,
        first_failure_at: parse_opt_time(first_failure_at)?,
        disabled_reason: row.try_get("disabled_reason").map_err(map_sqlx)?,
        created_at: parse_time(&created_at)?,
        updated_at: parse_time(&updated_at)?,
    })
}

// ---------------------------------------------------------------------------
// Deliveries
// ---------------------------------------------------------------------------

pub async fn create_pending(
    pool: &SqlitePool,
    endpoint_id: WebhookEndpointId,
    event_id: &str,
    event_type: &str,
    payload: &str,
) -> Result<WebhookDeliveryId> {
    let id = WebhookDeliveryId::new();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO outgoing_webhook_deliveries
         (id, endpoint_id, event_id, event_type, payload, status, attempts,
          next_attempt_at, lease_until, response_status, response_size, error,
          created_at, delivered_at, exhausted_at)
         VALUES (?, ?, ?, ?, ?, 'pending', 0, ?, NULL, NULL, NULL, NULL, ?, NULL, NULL)",
    )
    .bind(id.as_uuid().to_string())
    .bind(endpoint_id.as_uuid().to_string())
    .bind(event_id)
    .bind(event_type)
    .bind(payload)
    .bind(&now) // next_attempt_at = now (immediate first attempt)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(id)
}

pub async fn get_delivery(pool: &SqlitePool, id: WebhookDeliveryId) -> Result<WebhookDelivery> {
    let row = sqlx::query(
        "SELECT id, endpoint_id, event_id, event_type, payload, status, attempts,
                next_attempt_at, lease_until, response_status, response_size, error,
                created_at, delivered_at, exhausted_at
         FROM outgoing_webhook_deliveries WHERE id = ?",
    )
    .bind(id.as_uuid().to_string())
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx)?
    .ok_or(Error::NotFound)?;
    parse_delivery(&row)
}

pub async fn list_for_endpoint(
    pool: &SqlitePool,
    endpoint_id: WebhookEndpointId,
    limit: i64,
) -> Result<Vec<WebhookDelivery>> {
    let rows = sqlx::query(
        "SELECT id, endpoint_id, event_id, event_type, payload, status, attempts,
                next_attempt_at, lease_until, response_status, response_size, error,
                created_at, delivered_at, exhausted_at
         FROM outgoing_webhook_deliveries
         WHERE endpoint_id = ?
         ORDER BY created_at DESC
         LIMIT ?",
    )
    .bind(endpoint_id.as_uuid().to_string())
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;
    rows.iter().map(parse_delivery).collect()
}

/// Atomically claims up to `limit` due deliveries by flipping them to
/// `in_flight` with a lease. Single statement + WAL single-writer => no double
/// claim. The status/lease guard in the WHERE clause is load-bearing.
pub async fn claim_due(
    pool: &SqlitePool,
    now: DateTime<Utc>,
    lease_seconds: i64,
    limit: i64,
) -> Result<Vec<WebhookDelivery>> {
    let now_str = now.to_rfc3339();
    let lease_until = (now + chrono::Duration::seconds(lease_seconds)).to_rfc3339();
    let rows = sqlx::query(
        "UPDATE outgoing_webhook_deliveries
         SET status = 'in_flight', lease_until = ?
         WHERE id IN (
             SELECT id FROM outgoing_webhook_deliveries
             WHERE status = 'pending' AND next_attempt_at <= ?
             ORDER BY next_attempt_at
             LIMIT ?
         )
         RETURNING id, endpoint_id, event_id, event_type, payload, status, attempts,
                   next_attempt_at, lease_until, response_status, response_size, error,
                   created_at, delivered_at, exhausted_at",
    )
    .bind(&lease_until)
    .bind(&now_str)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;
    rows.iter().map(parse_delivery).collect()
}

/// Returns `in_flight` deliveries whose lease expired (worker crashed mid-send)
/// back to `pending` for re-attempt.
pub async fn reclaim_expired(pool: &SqlitePool, now: DateTime<Utc>) -> Result<u64> {
    let now_str = now.to_rfc3339();
    let res = sqlx::query(
        "UPDATE outgoing_webhook_deliveries
         SET status = 'pending', lease_until = NULL
         WHERE status = 'in_flight' AND lease_until < ?",
    )
    .bind(&now_str)
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(res.rows_affected())
}

pub async fn mark_success(
    pool: &SqlitePool,
    id: WebhookDeliveryId,
    response_status: Option<i64>,
    response_size: Option<i64>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE outgoing_webhook_deliveries
         SET status = 'success', attempts = attempts + 1, lease_until = NULL,
             response_status = ?, response_size = ?, delivered_at = ?
         WHERE id = ?",
    )
    .bind(response_status)
    .bind(response_size)
    .bind(&now)
    .bind(id.as_uuid().to_string())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

/// Marks a delivery as retryable: increments attempts, schedules the next
/// attempt, and returns to `pending`.
pub async fn mark_retryable(
    pool: &SqlitePool,
    id: WebhookDeliveryId,
    next_attempt_at: DateTime<Utc>,
    response_status: Option<i64>,
    error: &str,
) -> Result<()> {
    let next = next_attempt_at.to_rfc3339();
    sqlx::query(
        "UPDATE outgoing_webhook_deliveries
         SET status = 'pending', attempts = attempts + 1, lease_until = NULL,
             next_attempt_at = ?, response_status = ?, error = ?
         WHERE id = ?",
    )
    .bind(&next)
    .bind(response_status)
    .bind(error)
    .bind(id.as_uuid().to_string())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

/// Dead-letters a delivery (permanent failure or attempts exhausted).
pub async fn mark_exhausted(
    pool: &SqlitePool,
    id: WebhookDeliveryId,
    response_status: Option<i64>,
    error: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE outgoing_webhook_deliveries
         SET status = 'failed', attempts = attempts + 1, lease_until = NULL,
             response_status = ?, error = ?, exhausted_at = ?
         WHERE id = ?",
    )
    .bind(response_status)
    .bind(error)
    .bind(&now)
    .bind(id.as_uuid().to_string())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

/// Resets a delivery for manual redelivery (re-queues regardless of prior state).
pub async fn reset_for_redeliver(pool: &SqlitePool, id: WebhookDeliveryId) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let res = sqlx::query(
        "UPDATE outgoing_webhook_deliveries
         SET status = 'pending', lease_until = NULL, next_attempt_at = ?,
             error = NULL, exhausted_at = NULL
         WHERE id = ?",
    )
    .bind(&now)
    .bind(id.as_uuid().to_string())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    if res.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    Ok(())
}

/// Prunes old deliveries, but retains dead-letters (`exhausted_at IS NOT NULL`)
/// so users have time to inspect and redeliver them.
pub async fn prune_old(pool: &SqlitePool, before: DateTime<Utc>) -> Result<u64> {
    let cutoff = before.to_rfc3339();
    let res = sqlx::query(
        "DELETE FROM outgoing_webhook_deliveries
         WHERE created_at < ? AND exhausted_at IS NULL",
    )
    .bind(&cutoff)
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(res.rows_affected())
}

fn parse_delivery(row: &SqliteRow) -> Result<WebhookDelivery> {
    let id: String = row.try_get("id").map_err(map_sqlx)?;
    let endpoint_id: String = row.try_get("endpoint_id").map_err(map_sqlx)?;
    let status_str: String = row.try_get("status").map_err(map_sqlx)?;
    let next_attempt_at: Option<String> = row.try_get("next_attempt_at").map_err(map_sqlx)?;
    let lease_until: Option<String> = row.try_get("lease_until").map_err(map_sqlx)?;
    let created_at: String = row.try_get("created_at").map_err(map_sqlx)?;
    let delivered_at: Option<String> = row.try_get("delivered_at").map_err(map_sqlx)?;
    let exhausted_at: Option<String> = row.try_get("exhausted_at").map_err(map_sqlx)?;

    Ok(WebhookDelivery {
        id: WebhookDeliveryId::from_uuid(parse_uuid(&id)?),
        endpoint_id: WebhookEndpointId::from_uuid(parse_uuid(&endpoint_id)?),
        event_id: row.try_get("event_id").map_err(map_sqlx)?,
        event_type: row.try_get("event_type").map_err(map_sqlx)?,
        payload: row.try_get("payload").map_err(map_sqlx)?,
        status: DeliveryStatus::parse(&status_str)
            .ok_or_else(|| Error::Internal(format!("bad delivery status: {status_str}")))?,
        attempts: row.try_get("attempts").map_err(map_sqlx)?,
        next_attempt_at: parse_opt_time(next_attempt_at)?,
        lease_until: parse_opt_time(lease_until)?,
        response_status: row.try_get("response_status").map_err(map_sqlx)?,
        response_size: row.try_get("response_size").map_err(map_sqlx)?,
        error: row.try_get("error").map_err(map_sqlx)?,
        created_at: parse_time(&created_at)?,
        delivered_at: parse_opt_time(delivered_at)?,
        exhausted_at: parse_opt_time(exhausted_at)?,
    })
}

fn parse_uuid(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|e| Error::Internal(e.to_string()))
}

fn parse_time(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| Error::Internal(e.to_string()))
}

fn parse_opt_time(s: Option<String>) -> Result<Option<DateTime<Utc>>> {
    match s {
        Some(v) => Ok(Some(parse_time(&v)?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::MasterKey;
    use crate::pool::connect;

    struct Ctx {
        pool: SqlitePool,
        key: MasterKey,
        ws: WorkspaceId,
        _dir: &'static tempfile::TempDir,
    }

    async fn ctx() -> Ctx {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = Box::leak(Box::new(tmp));
        let pool = connect(&dir.path().join("test.db")).await.unwrap();
        let key = MasterKey::load_or_create(&dir.path().join("master.key")).unwrap();
        let user = crate::queries::auth::create_local_user(&pool, "tester")
            .await
            .unwrap();
        let ws = crate::queries::workspaces::create(&pool, "ws", "WS", user)
            .await
            .unwrap();
        Ctx {
            pool,
            key,
            ws: ws.id,
            _dir: dir,
        }
    }

    async fn make_endpoint(c: &Ctx, events: &[&str]) -> WebhookEndpointId {
        let events: Vec<String> = events.iter().map(|s| s.to_string()).collect();
        let ep = create(
            &c.pool,
            &c.key,
            c.ws,
            None,
            "webhook",
            "https://example.com/hook",
            &serde_json::json!({}),
            &serde_json::json!({ "signing_secret": "shh" }),
            &events,
        )
        .await
        .unwrap();
        ep.id
    }

    #[tokio::test]
    async fn credential_roundtrip() {
        let c = ctx().await;
        let id = make_endpoint(&c, &["*"]).await;
        let creds = credentials_for(&c.pool, &c.key, id).await.unwrap();
        assert_eq!(creds["signing_secret"], "shh");
    }

    #[tokio::test]
    async fn list_active_for_event_matches_exactly_and_wildcard() {
        let c = ctx().await;
        let wild = make_endpoint(&c, &["*"]).await;
        let exact = make_endpoint(&c, &["deployment.failed"]).await;
        let _other = make_endpoint(&c, &["backup.succeeded"]).await;

        let got = list_active_for_event(&c.pool, c.ws, None, "deployment.failed")
            .await
            .unwrap();
        let ids: Vec<_> = got.iter().map(|e| e.id).collect();
        assert!(ids.contains(&wild), "wildcard endpoint should match");
        assert!(ids.contains(&exact), "exact-match endpoint should match");
        assert_eq!(ids.len(), 2, "non-matching endpoint must be excluded");

        // Substring traps: a subscriber to `deployment.fail` must NOT match
        // `deployment.failed`.
        let near = make_endpoint(&c, &["deployment.fail"]).await;
        let got = list_active_for_event(&c.pool, c.ws, None, "deployment.failed")
            .await
            .unwrap();
        assert!(
            !got.iter().any(|e| e.id == near),
            "substring must not falsely match"
        );
    }

    #[tokio::test]
    async fn disabled_endpoint_excluded() {
        let c = ctx().await;
        let id = make_endpoint(&c, &["*"]).await;
        disable(&c.pool, id, "test").await.unwrap();
        let got = list_active_for_event(&c.pool, c.ws, None, "anything")
            .await
            .unwrap();
        assert!(got.is_empty());
        // enable() restores it and clears the failure counters.
        enable(&c.pool, id).await.unwrap();
        let got = list_active_for_event(&c.pool, c.ws, None, "anything")
            .await
            .unwrap();
        assert_eq!(got.len(), 1);
    }

    #[tokio::test]
    async fn claim_due_is_atomic_no_double_claim() {
        let c = ctx().await;
        let id = make_endpoint(&c, &["*"]).await;
        let _d1 = create_pending(&c.pool, id, "evt_1", "test", "{}")
            .await
            .unwrap();
        let _d2 = create_pending(&c.pool, id, "evt_2", "test", "{}")
            .await
            .unwrap();

        let now = Utc::now();
        let first = claim_due(&c.pool, now, 60, 10).await.unwrap();
        assert_eq!(first.len(), 2, "both due deliveries claimed");
        // A second claim finds nothing (they are in_flight, not pending).
        let second = claim_due(&c.pool, now, 60, 10).await.unwrap();
        assert!(second.is_empty(), "claimed rows must not be re-claimed");
    }

    #[tokio::test]
    async fn reclaim_expired_returns_in_flight_to_pending() {
        let c = ctx().await;
        let id = make_endpoint(&c, &["*"]).await;
        create_pending(&c.pool, id, "evt_1", "test", "{}")
            .await
            .unwrap();

        // Claim with a lease that is already expired relative to a future "now".
        let now = Utc::now();
        let claimed = claim_due(&c.pool, now, 1, 10).await.unwrap();
        assert_eq!(claimed.len(), 1);

        // No reclaim before the lease expires.
        let n = reclaim_expired(&c.pool, now).await.unwrap();
        assert_eq!(n, 0);
        // After the lease window, the row is reclaimed.
        let n = reclaim_expired(&c.pool, now + chrono::Duration::seconds(5))
            .await
            .unwrap();
        assert_eq!(n, 1);
        let due = claim_due(&c.pool, now + chrono::Duration::seconds(5), 60, 10)
            .await
            .unwrap();
        assert_eq!(due.len(), 1, "reclaimed delivery is claimable again");
    }

    #[tokio::test]
    async fn record_failure_then_success_resets_streak() {
        let c = ctx().await;
        let id = make_endpoint(&c, &["*"]).await;
        let (n1, first) = record_failure(&c.pool, id).await.unwrap();
        assert_eq!(n1, 1);
        assert!(first.is_some(), "first_failure_at set on first failure");
        let (n2, _) = record_failure(&c.pool, id).await.unwrap();
        assert_eq!(n2, 2);
        record_success(&c.pool, id).await.unwrap();
        let ep = get(&c.pool, id).await.unwrap();
        assert_eq!(ep.consecutive_failures, 0);
        assert!(ep.first_failure_at.is_none());
    }

    #[tokio::test]
    async fn prune_keeps_dead_letters() {
        let c = ctx().await;
        let id = make_endpoint(&c, &["*"]).await;
        let live = create_pending(&c.pool, id, "evt_live", "test", "{}")
            .await
            .unwrap();
        let dead = create_pending(&c.pool, id, "evt_dead", "test", "{}")
            .await
            .unwrap();
        mark_exhausted(&c.pool, dead, Some(500), "boom").await.unwrap();

        // Prune everything created before "now + 1d": the live (non-exhausted)
        // row goes, the dead-letter stays.
        let cutoff = Utc::now() + chrono::Duration::days(1);
        let removed = prune_old(&c.pool, cutoff).await.unwrap();
        assert_eq!(removed, 1);
        assert!(get_delivery(&c.pool, live).await.is_err());
        assert!(get_delivery(&c.pool, dead).await.is_ok());
    }
}
