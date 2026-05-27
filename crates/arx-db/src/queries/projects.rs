use super::map_sqlx;
use arx_core::ids::{ProjectId, WorkspaceId};
use arx_core::model::Project;
use arx_core::{Error, Result};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

pub async fn create(
    pool: &SqlitePool,
    workspace_id: WorkspaceId,
    slug: &str,
    name: &str,
) -> Result<Project> {
    arx_core::slug::validate("project slug", slug)?;
    let id = ProjectId::new();
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    let mut tx = pool.begin().await.map_err(map_sqlx)?;

    sqlx::query(
        "INSERT INTO projects (id, workspace_id, slug, name, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.as_uuid().to_string())
    .bind(workspace_id.as_uuid().to_string())
    .bind(slug)
    .bind(name)
    .bind(&now_str)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx)?;

    let env_id = arx_core::ids::EnvironmentId::new();
    sqlx::query(
        "INSERT INTO environments (id, project_id, slug, name, is_default, created_at)
         VALUES (?, ?, 'production', 'Production', 1, ?)",
    )
    .bind(env_id.as_uuid().to_string())
    .bind(id.as_uuid().to_string())
    .bind(&now_str)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx)?;

    tx.commit().await.map_err(map_sqlx)?;

    Ok(Project {
        id,
        workspace_id,
        slug: slug.to_string(),
        name: name.to_string(),
        created_at: now,
    })
}

pub async fn get_by_id(pool: &SqlitePool, id: ProjectId) -> Result<Project> {
    let row =
        sqlx::query("SELECT id, workspace_id, slug, name, created_at FROM projects WHERE id = ?")
            .bind(id.as_uuid().to_string())
            .fetch_optional(pool)
            .await
            .map_err(map_sqlx)?;
    parse(&row.ok_or(Error::NotFound)?)
}

pub async fn get_by_slug(
    pool: &SqlitePool,
    workspace_id: WorkspaceId,
    slug: &str,
) -> Result<Project> {
    let row = sqlx::query(
        "SELECT id, workspace_id, slug, name, created_at
         FROM projects WHERE workspace_id = ? AND slug = ?",
    )
    .bind(workspace_id.as_uuid().to_string())
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx)?;
    parse(&row.ok_or(Error::NotFound)?)
}

pub async fn list_in_workspace(
    pool: &SqlitePool,
    workspace_id: WorkspaceId,
) -> Result<Vec<Project>> {
    let rows = sqlx::query(
        "SELECT id, workspace_id, slug, name, created_at
         FROM projects WHERE workspace_id = ? ORDER BY created_at ASC",
    )
    .bind(workspace_id.as_uuid().to_string())
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;
    rows.iter().map(parse).collect()
}

fn parse(row: &sqlx::sqlite::SqliteRow) -> Result<Project> {
    let id_str: String = row.try_get("id").map_err(map_sqlx)?;
    let ws_str: String = row.try_get("workspace_id").map_err(map_sqlx)?;
    let slug: String = row.try_get("slug").map_err(map_sqlx)?;
    let name: String = row.try_get("name").map_err(map_sqlx)?;
    let created: String = row.try_get("created_at").map_err(map_sqlx)?;
    Ok(Project {
        id: ProjectId::from_uuid(
            Uuid::parse_str(&id_str).map_err(|e| Error::Internal(e.to_string()))?,
        ),
        workspace_id: WorkspaceId::from_uuid(
            Uuid::parse_str(&ws_str).map_err(|e| Error::Internal(e.to_string()))?,
        ),
        slug,
        name,
        created_at: DateTime::parse_from_rfc3339(&created)
            .map_err(|e| Error::Internal(e.to_string()))?
            .with_timezone(&Utc),
    })
}
