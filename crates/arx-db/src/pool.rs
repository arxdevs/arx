use arx_core::Result;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::path::Path;
use std::str::FromStr;

pub type Db = SqlitePool;

pub async fn connect(db_path: &Path) -> Result<Db> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let opts = SqliteConnectOptions::from_str(&url)
        .map_err(|e| arx_core::Error::Internal(format!("sqlite opts: {e}")))?
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await
        .map_err(|e| arx_core::Error::Internal(format!("sqlite connect: {e}")))?;

    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .map_err(|e| arx_core::Error::Internal(format!("migrate: {e}")))?;

    Ok(pool)
}
