use super::map_sqlx;
use crate::crypto::MasterKey;
use arx_core::ids::{EnvironmentId, ServiceId, VariableId};
use arx_core::{Error, Result};
use chrono::Utc;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct VariableListing {
    pub id: VariableId,
    pub key: String,

    pub plaintext: Option<String>,
    pub sealed: bool,
}

pub struct SetOutcome {
    pub replaced_sealed: bool,
}

pub async fn set(
    pool: &SqlitePool,
    key_master: &MasterKey,
    service_id: ServiceId,
    environment_id: EnvironmentId,
    key: &str,
    value: &str,
    sealed: bool,
) -> Result<SetOutcome> {
    let (ct, nonce) = key_master.encrypt(value.as_bytes())?;
    let now = Utc::now().to_rfc3339();

    let existing = sqlx::query(
        "SELECT id, sealed FROM variables
         WHERE service_id = ? AND environment_id = ? AND key = ?",
    )
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .bind(key)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx)?;

    let mut replaced_sealed = false;
    if let Some(row) = existing {
        let existing_sealed: i64 = row.try_get("sealed").map_err(map_sqlx)?;
        replaced_sealed = existing_sealed != 0;

        let effective_sealed = sealed || replaced_sealed;
        let id_str: String = row.try_get("id").map_err(map_sqlx)?;
        sqlx::query(
            "UPDATE variables
             SET value_ciphertext = ?, value_nonce = ?, sealed = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&ct)
        .bind(&nonce)
        .bind(if effective_sealed { 1 } else { 0 })
        .bind(&now)
        .bind(&id_str)
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    } else {
        let id = VariableId::new();
        sqlx::query(
            "INSERT INTO variables
             (id, service_id, environment_id, key, value_ciphertext, value_nonce, sealed, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_uuid().to_string())
        .bind(service_id.as_uuid().to_string())
        .bind(environment_id.as_uuid().to_string())
        .bind(key)
        .bind(&ct)
        .bind(&nonce)
        .bind(if sealed { 1 } else { 0 })
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    }
    Ok(SetOutcome { replaced_sealed })
}

pub async fn unset(
    pool: &SqlitePool,
    service_id: ServiceId,
    environment_id: EnvironmentId,
    key: &str,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM variables
         WHERE service_id = ? AND environment_id = ? AND key = ?",
    )
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .bind(key)
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

pub async fn list(
    pool: &SqlitePool,
    key_master: &MasterKey,
    service_id: ServiceId,
    environment_id: EnvironmentId,
) -> Result<Vec<VariableListing>> {
    let rows = sqlx::query(
        "SELECT id, key, value_ciphertext, value_nonce, sealed
         FROM variables WHERE service_id = ? AND environment_id = ?
         ORDER BY key ASC",
    )
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let id_str: String = r.try_get("id").map_err(map_sqlx)?;
        let key: String = r.try_get("key").map_err(map_sqlx)?;
        let ct: Vec<u8> = r.try_get("value_ciphertext").map_err(map_sqlx)?;
        let nonce: Vec<u8> = r.try_get("value_nonce").map_err(map_sqlx)?;
        let sealed_i: i64 = r.try_get("sealed").map_err(map_sqlx)?;
        let sealed = sealed_i != 0;

        let plaintext = if sealed {
            None
        } else {
            let bytes = key_master.decrypt(&ct, &nonce)?;
            Some(String::from_utf8(bytes).map_err(|e| Error::Internal(e.to_string()))?)
        };

        out.push(VariableListing {
            id: VariableId::from_uuid(
                Uuid::parse_str(&id_str).map_err(|e| Error::Internal(e.to_string()))?,
            ),
            key,
            plaintext,
            sealed,
        });
    }
    Ok(out)
}

pub async fn for_injection(
    pool: &SqlitePool,
    key_master: &MasterKey,
    service_id: ServiceId,
    environment_id: EnvironmentId,
) -> Result<Vec<(String, String)>> {
    let rows = sqlx::query(
        "SELECT key, value_ciphertext, value_nonce
         FROM variables WHERE service_id = ? AND environment_id = ?",
    )
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let key: String = r.try_get("key").map_err(map_sqlx)?;
        let ct: Vec<u8> = r.try_get("value_ciphertext").map_err(map_sqlx)?;
        let nonce: Vec<u8> = r.try_get("value_nonce").map_err(map_sqlx)?;
        let bytes = key_master.decrypt(&ct, &nonce)?;
        let value = String::from_utf8(bytes).map_err(|e| Error::Internal(e.to_string()))?;
        out.push((key, value));
    }
    Ok(out)
}
