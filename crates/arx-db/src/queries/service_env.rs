use super::map_sqlx;
use arx_core::ids::{EnvironmentId, ServiceId};
use arx_core::model::HealthcheckMode;
use arx_core::{Error, Result};
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Default)]
pub struct EnvConfig {
    pub cpu_limit: Option<f64>,
    pub memory_limit_mb: Option<i64>,
    pub healthcheck_mode: HealthcheckMode,
    pub healthcheck_path: Option<String>,
    pub healthcheck_timeout_seconds: i32,
}

pub async fn get(
    pool: &SqlitePool,
    service_id: ServiceId,
    environment_id: EnvironmentId,
) -> Result<EnvConfig> {
    let row = sqlx::query(
        "SELECT cpu_limit, memory_limit_mb, healthcheck_mode, healthcheck_path, healthcheck_timeout_seconds
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
    let healthcheck_path: Option<String> = row.try_get("healthcheck_path").ok();
    let healthcheck_mode = row
        .try_get::<String, _>("healthcheck_mode")
        .ok()
        .and_then(|s| HealthcheckMode::parse(&s))
        .unwrap_or_else(|| {
            if healthcheck_path
                .as_deref()
                .is_some_and(|p| !p.trim().is_empty())
            {
                HealthcheckMode::Http
            } else {
                HealthcheckMode::Tcp
            }
        });
    Ok(EnvConfig {
        cpu_limit: row.try_get("cpu_limit").ok(),
        memory_limit_mb: row.try_get("memory_limit_mb").ok(),
        healthcheck_mode,
        healthcheck_path,
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
    let mut sets: Vec<&'static str> = Vec::new();
    if patch.cpu_limit.is_some() {
        sets.push("cpu_limit = ?");
    }
    if patch.memory_limit_mb.is_some() {
        sets.push("memory_limit_mb = ?");
    }
    if patch.healthcheck_mode.is_some() {
        sets.push("healthcheck_mode = ?");
    }
    if patch.healthcheck_path.is_some() {
        sets.push("healthcheck_path = ?");
    }
    if patch.healthcheck_timeout_seconds.is_some() {
        sets.push("healthcheck_timeout_seconds = ?");
    }
    if sets.is_empty() {
        return Ok(());
    }

    let sql = format!(
        "UPDATE service_env_configs SET {} WHERE service_id = ? AND environment_id = ?",
        sets.join(", ")
    );
    let mut q = sqlx::query(&sql);
    if let Some(v) = patch.cpu_limit {
        q = q.bind(v);
    }
    if let Some(v) = patch.memory_limit_mb {
        q = q.bind(v);
    }
    if let Some(v) = patch.healthcheck_mode {
        q = q.bind(v.as_str());
    }
    if let Some(v) = patch.healthcheck_path {
        q = q.bind(v);
    }
    if let Some(v) = patch.healthcheck_timeout_seconds {
        q = q.bind(v);
    }
    q.bind(service_id.as_uuid().to_string())
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
    pub healthcheck_mode: Option<HealthcheckMode>,
    /// `Some(None)` serializes as JSON null to clear the path.
    pub healthcheck_path: Option<Option<String>>,
    pub healthcheck_timeout_seconds: Option<i32>,
}
