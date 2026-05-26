use super::map_sqlx;
use crate::crypto::MasterKey;
use arx_core::{Error, Result};
use chrono::Utc;
use sqlx::{Row, SqlitePool};

pub struct AppCreds {
    pub app_id: i64,
    pub slug: String,
    pub name: String,
    pub client_id: String,
    pub client_secret: String,
    pub webhook_secret: String,
    pub private_key_pem: String,
    pub html_url: String,
}

pub async fn put_app(pool: &SqlitePool, key: &MasterKey, creds: &AppCreds) -> Result<()> {
    let (cs_ct, cs_nonce) = key.encrypt(creds.client_secret.as_bytes())?;
    let (ws_ct, ws_nonce) = key.encrypt(creds.webhook_secret.as_bytes())?;
    let (pk_ct, pk_nonce) = key.encrypt(creds.private_key_pem.as_bytes())?;
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO github_app (
            id, app_id, slug, name, client_id,
            client_secret_ct, client_secret_nonce,
            webhook_secret_ct, webhook_secret_nonce,
            private_key_ct, private_key_nonce,
            html_url, created_at
         ) VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET
            app_id = excluded.app_id,
            slug = excluded.slug,
            name = excluded.name,
            client_id = excluded.client_id,
            client_secret_ct = excluded.client_secret_ct,
            client_secret_nonce = excluded.client_secret_nonce,
            webhook_secret_ct = excluded.webhook_secret_ct,
            webhook_secret_nonce = excluded.webhook_secret_nonce,
            private_key_ct = excluded.private_key_ct,
            private_key_nonce = excluded.private_key_nonce,
            html_url = excluded.html_url",
    )
    .bind(creds.app_id)
    .bind(&creds.slug)
    .bind(&creds.name)
    .bind(&creds.client_id)
    .bind(&cs_ct)
    .bind(&cs_nonce)
    .bind(&ws_ct)
    .bind(&ws_nonce)
    .bind(&pk_ct)
    .bind(&pk_nonce)
    .bind(&creds.html_url)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

pub async fn get_app(pool: &SqlitePool, key: &MasterKey) -> Result<Option<AppCreds>> {
    let row = sqlx::query(
        "SELECT app_id, slug, name, client_id,
                client_secret_ct, client_secret_nonce,
                webhook_secret_ct, webhook_secret_nonce,
                private_key_ct, private_key_nonce,
                html_url
         FROM github_app WHERE id = 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx)?;

    let Some(row) = row else { return Ok(None) };

    let client_secret_ct: Vec<u8> = row.try_get("client_secret_ct").map_err(map_sqlx)?;
    let client_secret_nonce: Vec<u8> = row.try_get("client_secret_nonce").map_err(map_sqlx)?;
    let webhook_secret_ct: Vec<u8> = row.try_get("webhook_secret_ct").map_err(map_sqlx)?;
    let webhook_secret_nonce: Vec<u8> = row.try_get("webhook_secret_nonce").map_err(map_sqlx)?;
    let private_key_ct: Vec<u8> = row.try_get("private_key_ct").map_err(map_sqlx)?;
    let private_key_nonce: Vec<u8> = row.try_get("private_key_nonce").map_err(map_sqlx)?;

    let client_secret = String::from_utf8(key.decrypt(&client_secret_ct, &client_secret_nonce)?)
        .map_err(|e| Error::Internal(e.to_string()))?;
    let webhook_secret = String::from_utf8(key.decrypt(&webhook_secret_ct, &webhook_secret_nonce)?)
        .map_err(|e| Error::Internal(e.to_string()))?;
    let private_key_pem = String::from_utf8(key.decrypt(&private_key_ct, &private_key_nonce)?)
        .map_err(|e| Error::Internal(e.to_string()))?;

    Ok(Some(AppCreds {
        app_id: row.try_get("app_id").map_err(map_sqlx)?,
        slug: row.try_get("slug").map_err(map_sqlx)?,
        name: row.try_get("name").map_err(map_sqlx)?,
        client_id: row.try_get("client_id").map_err(map_sqlx)?,
        client_secret,
        webhook_secret,
        private_key_pem,
        html_url: row.try_get("html_url").map_err(map_sqlx)?,
    }))
}
