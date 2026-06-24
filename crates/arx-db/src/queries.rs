pub mod audit;
pub mod auth;
pub mod backups;
pub mod deployments;
pub mod domains;
pub mod environments;
pub mod github;
pub mod members;
pub mod projects;
pub mod service_env;
pub mod services;
pub mod settings;
pub mod variables;
pub mod webhooks;
pub mod workspaces;

use arx_core::{Error, Result};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

pub(crate) fn map_sqlx(e: sqlx::Error) -> Error {
    match e {
        sqlx::Error::RowNotFound => Error::NotFound,
        sqlx::Error::Database(db) if db.is_unique_violation() => Error::AlreadyExists,
        other => Error::Internal(format!("sqlx: {other}")),
    }
}

pub(crate) trait RowExt {
    fn try_id<I: From<Uuid>>(&self, col: &str) -> Result<I>;
}

impl RowExt for SqliteRow {
    fn try_id<I: From<Uuid>>(&self, col: &str) -> Result<I> {
        let s: String = self.try_get(col).map_err(map_sqlx)?;
        let u = Uuid::parse_str(&s).map_err(|e| Error::Internal(e.to_string()))?;
        Ok(I::from(u))
    }
}
