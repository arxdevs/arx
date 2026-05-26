use super::{RowExt, map_sqlx};
use arx_core::ids::{DomainId, EnvironmentId, ServiceId};
use arx_core::model::{CertStatus, Domain};
use arx_core::{Error, Result};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};

pub async fn add(
    pool: &SqlitePool,
    service_id: ServiceId,
    environment_id: EnvironmentId,
    hostname: &str,
) -> Result<Domain> {
    let id = DomainId::new();
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO domains (id, service_id, environment_id, hostname, verified, cert_status, created_at)
         VALUES (?, ?, ?, ?, 0, 'pending', ?)",
    )
    .bind(id.as_uuid().to_string())
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .bind(hostname)
    .bind(now.to_rfc3339())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;

    Ok(Domain {
        id,
        service_id,
        environment_id,
        hostname: hostname.into(),
        verified: false,
        cert_status: CertStatus::Pending,
        created_at: now,
    })
}

pub async fn remove(pool: &SqlitePool, id: DomainId) -> Result<()> {
    sqlx::query("DELETE FROM domains WHERE id = ?")
        .bind(id.as_uuid().to_string())
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    Ok(())
}

pub async fn remove_scoped(pool: &SqlitePool, service_id: ServiceId, id: DomainId) -> Result<()> {
    let res = sqlx::query("DELETE FROM domains WHERE id = ? AND service_id = ?")
        .bind(id.as_uuid().to_string())
        .bind(service_id.as_uuid().to_string())
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    if res.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    Ok(())
}

pub async fn list_for_service_env(
    pool: &SqlitePool,
    service_id: ServiceId,
    environment_id: EnvironmentId,
) -> Result<Vec<Domain>> {
    let rows = sqlx::query(
        "SELECT id, service_id, environment_id, hostname, verified, cert_status, created_at
         FROM domains WHERE service_id = ? AND environment_id = ?
         ORDER BY created_at ASC",
    )
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;
    rows.iter().map(parse).collect()
}

pub async fn list_all_active(pool: &SqlitePool) -> Result<Vec<Domain>> {
    let rows = sqlx::query(
        "SELECT id, service_id, environment_id, hostname, verified, cert_status, created_at
         FROM domains",
    )
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;
    rows.iter().map(parse).collect()
}

fn parse(row: &sqlx::sqlite::SqliteRow) -> Result<Domain> {
    let hostname: String = row.try_get("hostname").map_err(map_sqlx)?;
    let verified: i64 = row.try_get("verified").map_err(map_sqlx)?;
    let cert_status_str: String = row.try_get("cert_status").map_err(map_sqlx)?;
    let created: String = row.try_get("created_at").map_err(map_sqlx)?;
    let cert_status = match cert_status_str.as_str() {
        "pending" => CertStatus::Pending,
        "issued" => CertStatus::Issued,
        "failed" => CertStatus::Failed,
        other => return Err(Error::Internal(format!("unknown cert_status: {other}"))),
    };
    Ok(Domain {
        id: row.try_id::<DomainId>("id")?,
        service_id: row.try_id::<ServiceId>("service_id")?,
        environment_id: row.try_id::<EnvironmentId>("environment_id")?,
        hostname,
        verified: verified != 0,
        cert_status,
        created_at: DateTime::parse_from_rfc3339(&created)
            .map_err(|e| Error::Internal(e.to_string()))?
            .with_timezone(&Utc),
    })
}
