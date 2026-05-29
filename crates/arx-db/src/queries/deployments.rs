use super::map_sqlx;
use arx_core::ids::{DeploymentId, EnvironmentId, ServiceId};
use arx_core::model::{Deployment, DeploymentStatus};
use arx_core::{Error, Result};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

pub async fn create_pending(
    pool: &SqlitePool,
    service_id: ServiceId,
    environment_id: EnvironmentId,
    image_ref: Option<&str>,
    commit_sha: Option<&str>,
    variables_snapshot: &serde_json::Value,
) -> Result<DeploymentId> {
    let id = DeploymentId::new();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO deployments
         (id, service_id, environment_id, status, image_ref, commit_sha,
          variables_snapshot, container_id, error, created_at, finished_at)
         VALUES (?, ?, ?, 'pending', ?, ?, ?, NULL, NULL, ?, NULL)",
    )
    .bind(id.as_uuid().to_string())
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .bind(image_ref)
    .bind(commit_sha)
    .bind(serde_json::to_string(variables_snapshot).unwrap_or_else(|_| "{}".into()))
    .bind(&now)
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(id)
}

pub async fn mark_interrupted_as_failed(
    pool: &SqlitePool,
) -> Result<Vec<(String, Option<String>)>> {
    let rows = sqlx::query(
        "SELECT id, container_id FROM deployments
         WHERE status IN ('pending', 'building', 'deploying')",
    )
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;
    let now = Utc::now().to_rfc3339();
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: String = row.try_get("id").map_err(map_sqlx)?;
        let container_id: Option<String> = row.try_get("container_id").ok().flatten();
        out.push((id, container_id));
    }
    if !rows.is_empty() {
        sqlx::query(
            "UPDATE deployments
             SET status = 'failed',
                 error = COALESCE(error, 'interrupted by daemon restart'),
                 finished_at = ?
             WHERE status IN ('pending', 'building', 'deploying')",
        )
        .bind(&now)
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    }
    Ok(out)
}

pub async fn update_status(
    pool: &SqlitePool,
    id: DeploymentId,
    status: DeploymentStatus,
    container_id: Option<&str>,
    error: Option<&str>,
    finished: bool,
) -> Result<()> {
    let finished_at = if finished {
        Some(Utc::now().to_rfc3339())
    } else {
        None
    };
    sqlx::query(
        "UPDATE deployments
         SET status = ?, container_id = COALESCE(?, container_id), error = ?, finished_at = COALESCE(?, finished_at)
         WHERE id = ?",
    )
    .bind(status.as_str())
    .bind(container_id)
    .bind(error)
    .bind(finished_at)
    .bind(id.as_uuid().to_string())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

pub async fn supersede_previous(
    pool: &SqlitePool,
    service_id: ServiceId,
    environment_id: EnvironmentId,
    new_deployment_id: DeploymentId,
) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT id, container_id FROM deployments
         WHERE service_id = ? AND environment_id = ?
           AND status = 'live' AND id != ?",
    )
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .bind(new_deployment_id.as_uuid().to_string())
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;

    let now = Utc::now().to_rfc3339();
    let mut containers = Vec::new();
    for row in rows {
        let id_str: String = row.try_get("id").map_err(map_sqlx)?;
        let container_id: Option<String> = row.try_get("container_id").ok();
        sqlx::query("UPDATE deployments SET status = 'superseded', finished_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&id_str)
            .execute(pool)
            .await
            .map_err(map_sqlx)?;
        if let Some(c) = container_id {
            containers.push(c);
        }
    }
    Ok(containers)
}

pub async fn get(pool: &SqlitePool, id: DeploymentId) -> Result<Deployment> {
    let row = sqlx::query(
        "SELECT id, service_id, environment_id, status, image_ref, commit_sha,
                variables_snapshot, container_id, error, created_at, finished_at
         FROM deployments WHERE id = ?",
    )
    .bind(id.as_uuid().to_string())
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx)?;
    parse(&row.ok_or(Error::NotFound)?)
}

pub async fn list_for_service_env(
    pool: &SqlitePool,
    service_id: ServiceId,
    environment_id: EnvironmentId,
    limit: i64,
) -> Result<Vec<Deployment>> {
    let rows = sqlx::query(
        "SELECT id, service_id, environment_id, status, image_ref, commit_sha,
                variables_snapshot, container_id, error, created_at, finished_at
         FROM deployments WHERE service_id = ? AND environment_id = ?
         ORDER BY created_at DESC LIMIT ?",
    )
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;
    rows.iter().map(parse).collect()
}

pub async fn current_live(
    pool: &SqlitePool,
    service_id: ServiceId,
    environment_id: EnvironmentId,
) -> Result<Option<Deployment>> {
    let row = sqlx::query(
        "SELECT id, service_id, environment_id, status, image_ref, commit_sha,
                variables_snapshot, container_id, error, created_at, finished_at
         FROM deployments WHERE service_id = ? AND environment_id = ? AND status = 'live'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx)?;
    row.map(|r| parse(&r)).transpose()
}

fn parse(row: &sqlx::sqlite::SqliteRow) -> Result<Deployment> {
    let id_str: String = row.try_get("id").map_err(map_sqlx)?;
    let sid: String = row.try_get("service_id").map_err(map_sqlx)?;
    let eid: String = row.try_get("environment_id").map_err(map_sqlx)?;
    let status_str: String = row.try_get("status").map_err(map_sqlx)?;
    let image_ref: Option<String> = row.try_get("image_ref").ok();
    let commit_sha: Option<String> = row.try_get("commit_sha").ok();
    let vs: String = row.try_get("variables_snapshot").map_err(map_sqlx)?;
    let container_id: Option<String> = row.try_get("container_id").ok();
    let error: Option<String> = row.try_get("error").ok();
    let created: String = row.try_get("created_at").map_err(map_sqlx)?;
    let finished: Option<String> = row.try_get("finished_at").ok();

    let status = match status_str.as_str() {
        "pending" => DeploymentStatus::Pending,
        "building" => DeploymentStatus::Building,
        "deploying" => DeploymentStatus::Deploying,
        "live" => DeploymentStatus::Live,
        "failed" => DeploymentStatus::Failed,
        "superseded" => DeploymentStatus::Superseded,
        "rolledback" => DeploymentStatus::Rolledback,
        other => return Err(Error::Internal(format!("unknown status: {other}"))),
    };

    let variables_snapshot: serde_json::Value =
        serde_json::from_str(&vs).unwrap_or(serde_json::Value::Object(Default::default()));

    Ok(Deployment {
        id: DeploymentId::from_uuid(
            Uuid::parse_str(&id_str).map_err(|e| Error::Internal(e.to_string()))?,
        ),
        service_id: ServiceId::from_uuid(
            Uuid::parse_str(&sid).map_err(|e| Error::Internal(e.to_string()))?,
        ),
        environment_id: EnvironmentId::from_uuid(
            Uuid::parse_str(&eid).map_err(|e| Error::Internal(e.to_string()))?,
        ),
        status,
        image_ref,
        commit_sha,
        variables_snapshot,
        container_id,
        error,
        created_at: DateTime::parse_from_rfc3339(&created)
            .map_err(|e| Error::Internal(e.to_string()))?
            .with_timezone(&Utc),
        finished_at: finished
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&Utc)),
    })
}
