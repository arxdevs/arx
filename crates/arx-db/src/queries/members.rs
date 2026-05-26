use super::map_sqlx;
use arx_core::ids::{UserId, WorkspaceId};
use arx_core::model::Role;
use arx_core::{Error, Result};
use chrono::Utc;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct MemberListing {
    pub user_id: UserId,
    pub display_name: String,
    pub github_login: Option<String>,
    pub role: Role,
}

pub async fn list(pool: &SqlitePool, workspace_id: WorkspaceId) -> Result<Vec<MemberListing>> {
    let rows = sqlx::query(
        "SELECT m.user_id, m.role, u.display_name, u.github_login
         FROM workspace_members m
         JOIN users u ON u.id = m.user_id
         WHERE m.workspace_id = ?
         ORDER BY m.created_at ASC",
    )
    .bind(workspace_id.as_uuid().to_string())
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let uid_str: String = r.try_get("user_id").map_err(map_sqlx)?;
        let role_str: String = r.try_get("role").map_err(map_sqlx)?;
        let role = match role_str.as_str() {
            "admin" => Role::Admin,
            "member" => Role::Member,
            other => return Err(Error::Internal(format!("unknown role: {other}"))),
        };
        out.push(MemberListing {
            user_id: UserId::from_uuid(
                Uuid::parse_str(&uid_str).map_err(|e| Error::Internal(e.to_string()))?,
            ),
            display_name: r.try_get("display_name").map_err(map_sqlx)?,
            github_login: r.try_get("github_login").ok(),
            role,
        });
    }
    Ok(out)
}

pub async fn invite_or_add(
    pool: &SqlitePool,
    workspace_id: WorkspaceId,
    inviter: UserId,
    github_login: &str,
    role: Role,
) -> Result<()> {
    let role_str = role.as_str();
    let now = Utc::now().to_rfc3339();

    let user_row = sqlx::query("SELECT id FROM users WHERE github_login = ?")
        .bind(github_login)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx)?;

    match user_row {
        Some(row) => {
            let uid_str: String = row.try_get("id").map_err(map_sqlx)?;
            let id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO workspace_members (id, workspace_id, user_id, role, created_at)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id.to_string())
            .bind(workspace_id.as_uuid().to_string())
            .bind(&uid_str)
            .bind(role_str)
            .bind(&now)
            .execute(pool)
            .await
            .map_err(map_sqlx)?;
        }
        None => {
            let id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO workspace_invites (id, workspace_id, github_login, role, invited_by, created_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(id.to_string())
            .bind(workspace_id.as_uuid().to_string())
            .bind(github_login)
            .bind(role_str)
            .bind(inviter.as_uuid().to_string())
            .bind(&now)
            .execute(pool)
            .await
            .map_err(map_sqlx)?;
        }
    }
    Ok(())
}

pub async fn remove(pool: &SqlitePool, workspace_id: WorkspaceId, user_id: UserId) -> Result<()> {
    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = ? AND user_id = ?")
        .bind(workspace_id.as_uuid().to_string())
        .bind(user_id.as_uuid().to_string())
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    Ok(())
}
