use super::map_sqlx;
use arx_core::Result;
use arx_core::ids::UserId;
use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

pub async fn write(
    pool: &SqlitePool,
    actor: Option<UserId>,
    action: &str,
    target: &str,
    metadata: serde_json::Value,
) -> Result<()> {
    let id = Uuid::now_v7();
    let now = Utc::now().to_rfc3339();
    let metadata_str = serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string());

    sqlx::query(
        "INSERT INTO audit_logs (id, actor_id, action, target, metadata, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(actor.map(|u| u.as_uuid().to_string()))
    .bind(action)
    .bind(target)
    .bind(&metadata_str)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

pub async fn prune(pool: &SqlitePool, retain: chrono::Duration) -> Result<u64> {
    let cutoff = (Utc::now() - retain).to_rfc3339();
    let res = sqlx::query("DELETE FROM audit_logs WHERE created_at < ?")
        .bind(&cutoff)
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    Ok(res.rows_affected())
}
