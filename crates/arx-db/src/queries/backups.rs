use super::map_sqlx;
use arx_core::ids::ServiceId;
use arx_core::{Error, Result};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct BackupSchedule {
    pub service_id: ServiceId,
    pub cron_expression: String,
    pub retention_count: i32,
    pub storage: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct BackupRecord {
    pub id: Uuid,
    pub service_id: ServiceId,
    pub size_bytes: i64,
    pub storage_uri: String,
    pub created_at: DateTime<Utc>,
}

pub async fn upsert_schedule(
    pool: &SqlitePool,
    service_id: ServiceId,
    cron_expression: &str,
    retention_count: i32,
    storage: &str,
    enabled: bool,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO backup_schedules (service_id, cron_expression, retention_count, storage, enabled)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (service_id) DO UPDATE SET
            cron_expression = excluded.cron_expression,
            retention_count = excluded.retention_count,
            storage = excluded.storage,
            enabled = excluded.enabled",
    )
    .bind(service_id.as_uuid().to_string())
    .bind(cron_expression)
    .bind(retention_count)
    .bind(storage)
    .bind(enabled as i64)
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

pub async fn get_schedule(
    pool: &SqlitePool,
    service_id: ServiceId,
) -> Result<Option<BackupSchedule>> {
    let row = sqlx::query(
        "SELECT cron_expression, retention_count, storage, enabled
         FROM backup_schedules WHERE service_id = ?",
    )
    .bind(service_id.as_uuid().to_string())
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx)?;
    let Some(r) = row else { return Ok(None) };
    Ok(Some(BackupSchedule {
        service_id,
        cron_expression: r.try_get("cron_expression").map_err(map_sqlx)?,
        retention_count: r
            .try_get::<i64, _>("retention_count")
            .map(|n| n as i32)
            .unwrap_or(7),
        storage: r.try_get("storage").map_err(map_sqlx)?,
        enabled: r
            .try_get::<i64, _>("enabled")
            .map(|n| n != 0)
            .unwrap_or(true),
    }))
}

pub async fn list_all_enabled(pool: &SqlitePool) -> Result<Vec<BackupSchedule>> {
    let rows = sqlx::query(
        "SELECT service_id, cron_expression, retention_count, storage, enabled
         FROM backup_schedules WHERE enabled = 1",
    )
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let svc_id: String = r.try_get("service_id").map_err(map_sqlx)?;
        out.push(BackupSchedule {
            service_id: ServiceId::from_uuid(
                Uuid::parse_str(&svc_id).map_err(|e| Error::Internal(e.to_string()))?,
            ),
            cron_expression: r.try_get("cron_expression").map_err(map_sqlx)?,
            retention_count: r
                .try_get::<i64, _>("retention_count")
                .map(|n| n as i32)
                .unwrap_or(7),
            storage: r.try_get("storage").map_err(map_sqlx)?,
            enabled: true,
        });
    }
    Ok(out)
}

pub async fn record(
    pool: &SqlitePool,
    service_id: ServiceId,
    size_bytes: i64,
    storage_uri: &str,
) -> Result<Uuid> {
    let id = Uuid::now_v7();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO backups (id, service_id, size_bytes, storage_uri, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(service_id.as_uuid().to_string())
    .bind(size_bytes)
    .bind(storage_uri)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(id)
}

pub async fn list_for_service(
    pool: &SqlitePool,
    service_id: ServiceId,
    limit: i64,
) -> Result<Vec<BackupRecord>> {
    let rows = sqlx::query(
        "SELECT id, service_id, size_bytes, storage_uri, created_at
         FROM backups WHERE service_id = ? ORDER BY created_at DESC LIMIT ?",
    )
    .bind(service_id.as_uuid().to_string())
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let id_str: String = r.try_get("id").map_err(map_sqlx)?;
        let svc_str: String = r.try_get("service_id").map_err(map_sqlx)?;
        let created: String = r.try_get("created_at").map_err(map_sqlx)?;
        out.push(BackupRecord {
            id: Uuid::parse_str(&id_str).map_err(|e| Error::Internal(e.to_string()))?,
            service_id: ServiceId::from_uuid(
                Uuid::parse_str(&svc_str).map_err(|e| Error::Internal(e.to_string()))?,
            ),
            size_bytes: r.try_get("size_bytes").map_err(map_sqlx)?,
            storage_uri: r.try_get("storage_uri").map_err(map_sqlx)?,
            created_at: DateTime::parse_from_rfc3339(&created)
                .map_err(|e| Error::Internal(e.to_string()))?
                .with_timezone(&Utc),
        });
    }
    Ok(out)
}

pub async fn prune(pool: &SqlitePool, service_id: ServiceId, keep: i32) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT id, storage_uri FROM backups WHERE service_id = ?
         ORDER BY created_at DESC LIMIT -1 OFFSET ?",
    )
    .bind(service_id.as_uuid().to_string())
    .bind(keep as i64)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;

    let mut uris = Vec::with_capacity(rows.len());
    for r in rows {
        let id: String = r.try_get("id").map_err(map_sqlx)?;
        let uri: String = r.try_get("storage_uri").map_err(map_sqlx)?;
        sqlx::query("DELETE FROM backups WHERE id = ?")
            .bind(&id)
            .execute(pool)
            .await
            .map_err(map_sqlx)?;
        uris.push(uri);
    }
    Ok(uris)
}
