use super::map_sqlx;
use arx_core::ids::{EnvironmentId, ServiceId};
use arx_core::{Error, Result};
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Default)]
pub struct EnvConfig {
    pub cpu_limit: Option<f64>,
    pub memory_limit_mb: Option<i64>,
    pub healthcheck_path: Option<String>,
    pub healthcheck_timeout_seconds: i32,
}

pub async fn get(
    pool: &SqlitePool,
    service_id: ServiceId,
    environment_id: EnvironmentId,
) -> Result<EnvConfig> {
    let row = sqlx::query(
        "SELECT cpu_limit, memory_limit_mb, healthcheck_path, healthcheck_timeout_seconds
         FROM service_env_configs WHERE service_id = ? AND environment_id = ?",
    )
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx)?;

    let Some(row) = row else {
        return Err(Error::NotFound);
    };
    Ok(EnvConfig {
        cpu_limit: row.try_get("cpu_limit").ok(),
        memory_limit_mb: row.try_get("memory_limit_mb").ok(),
        healthcheck_path: row.try_get("healthcheck_path").ok(),
        healthcheck_timeout_seconds: row
            .try_get::<i64, _>("healthcheck_timeout_seconds")
            .map(|n| n as i32)
            .unwrap_or(60),
    })
}

pub async fn update(
    pool: &SqlitePool,
    service_id: ServiceId,
    environment_id: EnvironmentId,
    patch: EnvConfigPatch,
) -> Result<()> {
    sqlx::query(
        "UPDATE service_env_configs SET
            cpu_limit = COALESCE(?, cpu_limit),
            memory_limit_mb = COALESCE(?, memory_limit_mb),
            healthcheck_path = COALESCE(?, healthcheck_path),
            healthcheck_timeout_seconds = COALESCE(?, healthcheck_timeout_seconds)
         WHERE service_id = ? AND environment_id = ?",
    )
    .bind(patch.cpu_limit)
    .bind(patch.memory_limit_mb)
    .bind(patch.healthcheck_path)
    .bind(patch.healthcheck_timeout_seconds)
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct EnvConfigPatch {
    pub cpu_limit: Option<f64>,
    pub memory_limit_mb: Option<i64>,
    pub healthcheck_path: Option<String>,
    pub healthcheck_timeout_seconds: Option<i32>,
}
