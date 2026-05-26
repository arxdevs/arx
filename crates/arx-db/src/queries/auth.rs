use super::map_sqlx;
use arx_core::ids::{UserId, WorkspaceId};
use arx_core::{Error, Result};
use chrono::{DateTime, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AuthedUser {
    pub user_id: UserId,
    pub display_name: String,
    pub github_login: Option<String>,
    pub session_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct CreatedSession {
    pub user_id: UserId,
    pub session_id: Uuid,
    pub token_plaintext: String,
}

const TOKEN_BYTES: usize = 32;

pub fn generate_token() -> String {
    let mut buf = [0u8; TOKEN_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut buf);

    hex::encode(buf)
}

pub fn hash_token(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    hex::encode(h.finalize())
}

pub async fn create_local_user(pool: &SqlitePool, display_name: &str) -> Result<UserId> {
    let id = UserId::new();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO users (id, github_login, github_user_id, display_name, avatar_url, created_at)
         VALUES (?, NULL, NULL, ?, NULL, ?)",
    )
    .bind(id.as_uuid().to_string())
    .bind(display_name)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(id)
}

pub async fn upsert_github_user(
    pool: &SqlitePool,
    github_user_id: i64,
    github_login: &str,
    display_name: &str,
    avatar_url: Option<&str>,
) -> Result<(UserId, bool)> {
    let now = Utc::now().to_rfc3339();
    let existing = sqlx::query("SELECT id FROM users WHERE github_user_id = ?")
        .bind(github_user_id)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx)?;

    if let Some(row) = existing {
        let id_str: String = row.try_get("id").map_err(map_sqlx)?;
        let uuid = Uuid::parse_str(&id_str).map_err(|e| Error::Internal(e.to_string()))?;
        sqlx::query(
            "UPDATE users SET github_login = ?, display_name = ?, avatar_url = ? WHERE id = ?",
        )
        .bind(github_login)
        .bind(display_name)
        .bind(avatar_url)
        .bind(&id_str)
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
        Ok((UserId::from_uuid(uuid), false))
    } else {
        let id = UserId::new();
        sqlx::query(
            "INSERT INTO users (id, github_login, github_user_id, display_name, avatar_url, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_uuid().to_string())
        .bind(github_login)
        .bind(github_user_id)
        .bind(display_name)
        .bind(avatar_url)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
        Ok((id, true))
    }
}

pub async fn count_users(pool: &SqlitePool) -> Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM users")
        .fetch_one(pool)
        .await
        .map_err(map_sqlx)?;
    row.try_get::<i64, _>("n").map_err(map_sqlx)
}

pub async fn issue_session(
    pool: &SqlitePool,
    user_id: UserId,
    label: Option<&str>,
) -> Result<CreatedSession> {
    let token = generate_token();
    let hash = hash_token(&token);
    let id = Uuid::now_v7();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, label, created_at, last_used_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, NULL)",
    )
    .bind(id.to_string())
    .bind(user_id.as_uuid().to_string())
    .bind(&hash)
    .bind(label)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(map_sqlx)?;

    Ok(CreatedSession {
        user_id,
        session_id: id,
        token_plaintext: token,
    })
}

pub async fn authenticate(pool: &SqlitePool, token: &str) -> Result<AuthedUser> {
    let hash = hash_token(token);
    let row = sqlx::query(
        "SELECT s.id AS session_id, s.user_id AS user_id, u.display_name, u.github_login, s.expires_at
         FROM sessions s
         JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = ?",
    )
    .bind(&hash)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx)?;

    let row = row.ok_or(Error::Unauthorized)?;

    if let Ok(Some(exp)) = row.try_get::<Option<String>, _>("expires_at") {
        if let Ok(parsed) = DateTime::parse_from_rfc3339(&exp) {
            if parsed.with_timezone(&Utc) < Utc::now() {
                return Err(Error::Unauthorized);
            }
        }
    }

    let session_id: String = row.try_get("session_id").map_err(map_sqlx)?;
    let user_id_str: String = row.try_get("user_id").map_err(map_sqlx)?;
    let display_name: String = row.try_get("display_name").map_err(map_sqlx)?;
    let github_login: Option<String> = row.try_get("github_login").ok();

    let _ = sqlx::query("UPDATE sessions SET last_used_at = ? WHERE id = ?")
        .bind(Utc::now().to_rfc3339())
        .bind(&session_id)
        .execute(pool)
        .await;

    let session_uuid = Uuid::parse_str(&session_id).map_err(|e| Error::Internal(e.to_string()))?;
    let user_uuid = Uuid::parse_str(&user_id_str).map_err(|e| Error::Internal(e.to_string()))?;
    Ok(AuthedUser {
        user_id: UserId::from_uuid(user_uuid),
        display_name,
        github_login,
        session_id: session_uuid,
    })
}

pub async fn revoke_session(pool: &SqlitePool, session_id: Uuid) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = ?")
        .bind(session_id.to_string())
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    Ok(())
}

pub async fn workspace_role(
    pool: &SqlitePool,
    workspace_id: WorkspaceId,
    user_id: UserId,
) -> Result<arx_core::model::Role> {
    let row =
        sqlx::query("SELECT role FROM workspace_members WHERE workspace_id = ? AND user_id = ?")
            .bind(workspace_id.as_uuid().to_string())
            .bind(user_id.as_uuid().to_string())
            .fetch_optional(pool)
            .await
            .map_err(map_sqlx)?;

    let row = row.ok_or(Error::Forbidden)?;
    let role_str: String = row.try_get("role").map_err(map_sqlx)?;
    match role_str.as_str() {
        "admin" => Ok(arx_core::model::Role::Admin),
        "member" => Ok(arx_core::model::Role::Member),
        other => Err(Error::Internal(format!("unknown role: {other}"))),
    }
}
