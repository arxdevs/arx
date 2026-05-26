use super::map_sqlx;
use arx_core::ids::{UserId, WorkspaceId};
use arx_core::model::{Role, Workspace};
use arx_core::{Error, Result};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

pub async fn create(
    pool: &SqlitePool,
    slug: &str,
    name: &str,
    owner_user_id: UserId,
) -> Result<Workspace> {
    let id = WorkspaceId::new();
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    let mut tx = pool.begin().await.map_err(map_sqlx)?;

    sqlx::query("INSERT INTO workspaces (id, slug, name, created_at) VALUES (?, ?, ?, ?)")
        .bind(id.as_uuid().to_string())
        .bind(slug)
        .bind(name)
        .bind(&now_str)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;

    let member_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO workspace_members (id, workspace_id, user_id, role, created_at)
         VALUES (?, ?, ?, 'admin', ?)",
    )
    .bind(member_id.to_string())
    .bind(id.as_uuid().to_string())
    .bind(owner_user_id.as_uuid().to_string())
    .bind(&now_str)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx)?;

    tx.commit().await.map_err(map_sqlx)?;

    Ok(Workspace {
        id,
        slug: slug.to_string(),
        name: name.to_string(),
        created_at: now,
    })
}

pub async fn get_by_id(pool: &SqlitePool, id: WorkspaceId) -> Result<Workspace> {
    let row = sqlx::query("SELECT id, slug, name, created_at FROM workspaces WHERE id = ?")
        .bind(id.as_uuid().to_string())
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx)?;
    parse_workspace_row(&row.ok_or(Error::NotFound)?)
}

pub async fn get_by_slug(pool: &SqlitePool, slug: &str) -> Result<Workspace> {
    let row = sqlx::query("SELECT id, slug, name, created_at FROM workspaces WHERE slug = ?")
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx)?;
    let row = row.ok_or(Error::NotFound)?;
    parse_workspace_row(&row)
}

pub async fn list_for_user(pool: &SqlitePool, user_id: UserId) -> Result<Vec<(Workspace, Role)>> {
    let rows = sqlx::query(
        "SELECT w.id, w.slug, w.name, w.created_at, m.role
         FROM workspaces w
         JOIN workspace_members m ON m.workspace_id = w.id
         WHERE m.user_id = ?
         ORDER BY w.created_at ASC",
    )
    .bind(user_id.as_uuid().to_string())
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let ws = parse_workspace_row(&r)?;
        let role_str: String = r.try_get("role").map_err(map_sqlx)?;
        let role = match role_str.as_str() {
            "admin" => Role::Admin,
            "member" => Role::Member,
            other => return Err(Error::Internal(format!("unknown role: {other}"))),
        };
        out.push((ws, role));
    }
    Ok(out)
}

fn parse_workspace_row(row: &sqlx::sqlite::SqliteRow) -> Result<Workspace> {
    let id_str: String = row.try_get("id").map_err(map_sqlx)?;
    let slug: String = row.try_get("slug").map_err(map_sqlx)?;
    let name: String = row.try_get("name").map_err(map_sqlx)?;
    let created_at: String = row.try_get("created_at").map_err(map_sqlx)?;
    let uuid = Uuid::parse_str(&id_str).map_err(|e| Error::Internal(e.to_string()))?;
    let created = DateTime::parse_from_rfc3339(&created_at)
        .map_err(|e| Error::Internal(e.to_string()))?
        .with_timezone(&Utc);
    Ok(Workspace {
        id: WorkspaceId::from_uuid(uuid),
        slug,
        name,
        created_at: created,
    })
}
