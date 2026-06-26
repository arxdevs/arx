use super::map_sqlx;
use arx_core::ids::{EnvironmentId, ProjectId};
use arx_core::model::Environment;
use arx_core::{Error, Result};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

pub async fn create(
    pool: &SqlitePool,
    project_id: ProjectId,
    slug: &str,
    name: &str,
) -> Result<Environment> {
    arx_core::slug::validate("environment slug", slug)?;
    let id = EnvironmentId::new();
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(map_sqlx)?;

    sqlx::query(
        "INSERT INTO environments (id, project_id, slug, name, is_default, created_at)
         VALUES (?, ?, ?, ?, 0, ?)",
    )
    .bind(id.as_uuid().to_string())
    .bind(project_id.as_uuid().to_string())
    .bind(slug)
    .bind(name)
    .bind(now.to_rfc3339())
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx)?;

    let service_rows = sqlx::query("SELECT id FROM services WHERE project_id = ?")
        .bind(project_id.as_uuid().to_string())
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx)?;
    for service_row in service_rows {
        let service_id: String = service_row.try_get("id").map_err(map_sqlx)?;
        sqlx::query(
            "INSERT INTO service_env_configs
             (service_id, environment_id, cpu_limit, memory_limit_mb,
              healthcheck_mode, healthcheck_path, healthcheck_timeout_seconds, current_deployment_id)
             VALUES (?, ?, NULL, NULL, 'tcp', NULL, 60, NULL)",
        )
        .bind(service_id)
        .bind(id.as_uuid().to_string())
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
    }

    tx.commit().await.map_err(map_sqlx)?;
    Ok(Environment {
        id,
        project_id,
        slug: slug.into(),
        name: name.into(),
        is_default: false,
        created_at: now,
    })
}

pub async fn rename(pool: &SqlitePool, id: EnvironmentId, name: &str) -> Result<()> {
    let res = sqlx::query("UPDATE environments SET name = ? WHERE id = ?")
        .bind(name)
        .bind(id.as_uuid().to_string())
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    if res.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    Ok(())
}

pub async fn get_by_id(pool: &SqlitePool, id: EnvironmentId) -> Result<Environment> {
    let row = sqlx::query(
        "SELECT id, project_id, slug, name, is_default, created_at FROM environments WHERE id = ?",
    )
    .bind(id.as_uuid().to_string())
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx)?;
    parse(&row.ok_or(Error::NotFound)?)
}

pub async fn get_by_slug(
    pool: &SqlitePool,
    project_id: ProjectId,
    slug: &str,
) -> Result<Environment> {
    let row = sqlx::query(
        "SELECT id, project_id, slug, name, is_default, created_at
         FROM environments WHERE project_id = ? AND slug = ?",
    )
    .bind(project_id.as_uuid().to_string())
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx)?;
    parse(&row.ok_or(Error::NotFound)?)
}

pub async fn list_in_project(pool: &SqlitePool, project_id: ProjectId) -> Result<Vec<Environment>> {
    let rows = sqlx::query(
        "SELECT id, project_id, slug, name, is_default, created_at
         FROM environments WHERE project_id = ? ORDER BY created_at ASC",
    )
    .bind(project_id.as_uuid().to_string())
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;
    rows.iter().map(parse).collect()
}

fn parse(row: &sqlx::sqlite::SqliteRow) -> Result<Environment> {
    let id: String = row.try_get("id").map_err(map_sqlx)?;
    let pid: String = row.try_get("project_id").map_err(map_sqlx)?;
    let slug: String = row.try_get("slug").map_err(map_sqlx)?;
    let name: String = row.try_get("name").map_err(map_sqlx)?;
    let is_default: i64 = row.try_get("is_default").map_err(map_sqlx)?;
    let created: String = row.try_get("created_at").map_err(map_sqlx)?;
    Ok(Environment {
        id: EnvironmentId::from_uuid(
            Uuid::parse_str(&id).map_err(|e| Error::Internal(e.to_string()))?,
        ),
        project_id: ProjectId::from_uuid(
            Uuid::parse_str(&pid).map_err(|e| Error::Internal(e.to_string()))?,
        ),
        slug,
        name,
        is_default: is_default != 0,
        created_at: DateTime::parse_from_rfc3339(&created)
            .map_err(|e| Error::Internal(e.to_string()))?
            .with_timezone(&Utc),
    })
}
