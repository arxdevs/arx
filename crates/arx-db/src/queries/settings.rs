use super::map_sqlx;
use arx_core::Result;
use chrono::Utc;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Default)]
pub struct ServerSettings {
    pub admin_domain: Option<String>,
    pub acme_email: Option<String>,
    pub public_ip: Option<String>,
}

pub async fn get(pool: &SqlitePool) -> Result<ServerSettings> {
    let row =
        sqlx::query("SELECT admin_domain, acme_email, public_ip FROM server_settings WHERE id = 1")
            .fetch_optional(pool)
            .await
            .map_err(map_sqlx)?;
    let Some(row) = row else {
        return Ok(ServerSettings::default());
    };
    Ok(ServerSettings {
        admin_domain: row.try_get("admin_domain").ok(),
        acme_email: row.try_get("acme_email").ok(),
        public_ip: row.try_get("public_ip").ok(),
    })
}

pub async fn set_admin_domain(pool: &SqlitePool, value: Option<&str>) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE server_settings SET admin_domain = ?, updated_at = ? WHERE id = 1")
        .bind(value)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    Ok(())
}

pub async fn set_acme_email(pool: &SqlitePool, value: Option<&str>) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE server_settings SET acme_email = ?, updated_at = ? WHERE id = 1")
        .bind(value)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    Ok(())
}

pub async fn set_public_ip(pool: &SqlitePool, value: Option<&str>) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE server_settings SET public_ip = ?, updated_at = ? WHERE id = 1")
        .bind(value)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    Ok(())
}
