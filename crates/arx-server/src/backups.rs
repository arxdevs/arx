use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use arx_core::ids::ServiceId;
use arx_core::model::{DbTemplate, Service, ServiceSource};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{info, warn};

pub async fn backup_now(app: &AppState, service: &Service) -> ApiResult<BackupReport> {
    let template = template_for(service)?;
    let container = active_container(app, service).await?;

    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let dir = app
        .config
        .paths
        .backups_dir
        .join(service.id.as_uuid().to_string());
    std::fs::create_dir_all(&dir).map_err(|e| ApiError::internal(e.to_string()))?;
    let path = dir.join(format!("{timestamp}.dump"));

    let user = decoded_var(app, service, "DATABASE_USER").await;
    let db = decoded_var(app, service, "DATABASE_NAME").await;
    let password = decoded_var(app, service, "DATABASE_PASSWORD").await;

    let outfile = std::fs::File::create(&path).map_err(|e| ApiError::internal(e.to_string()))?;

    let mut cmd = Command::new("docker");
    cmd.arg("exec").arg("-i");
    let mut env_args: Vec<(&str, String)> = Vec::new();

    match template {
        DbTemplate::Postgres => {
            let user = user.clone().unwrap_or_else(|| "postgres".to_string());
            let db = db.clone().unwrap_or_else(|| "postgres".to_string());
            if let Some(p) = &password {
                env_args.push(("-e", format!("PGPASSWORD={p}")));
            }
            for (flag, val) in &env_args {
                cmd.arg(flag).arg(val);
            }
            cmd.arg(&container)
                .arg("pg_dump")
                .arg("-U")
                .arg(&user)
                .arg(&db);
        }
        DbTemplate::Mysql => {
            let db = db.clone().unwrap_or_else(|| "".to_string());
            // Pass the password via MYSQL_PWD env rather than -p<pw> so it does
            // not leak into the container's process list.
            if let Some(p) = &password {
                cmd.arg("-e").arg(format!("MYSQL_PWD={p}"));
            }
            cmd.arg(&container)
                .arg("mysqldump")
                .arg("-u")
                .arg("root")
                .arg(db);
        }
        DbTemplate::Mongodb => {
            cmd.arg(&container).arg("mongodump").arg("--archive");
        }
        DbTemplate::Redis => {
            cmd.arg("redis-cli").arg("--rdb").arg("/data/dump.rdb");
        }
    }

    let dump_result: ApiResult<()> = async {
        if matches!(template, DbTemplate::Redis) {
            let status = Command::new("docker")
                .args(["exec", &container, "redis-cli", "save"])
                .status()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            if !status.success() {
                return Err(ApiError::internal("redis save failed"));
            }
            let cp_status = Command::new("docker")
                .arg("cp")
                .arg(format!("{container}:/data/dump.rdb"))
                .arg(&path)
                .status()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            if !cp_status.success() {
                return Err(ApiError::internal("docker cp failed"));
            }
        } else {
            cmd.stdout(Stdio::from(outfile));
            cmd.stdin(Stdio::null());
            let status = cmd
                .status()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            if !status.success() {
                return Err(ApiError::internal(format!(
                    "{:?} failed: exit {}",
                    template,
                    status.code().unwrap_or(-1)
                )));
            }
        }
        Ok(())
    }
    .await;

    // On any failure, drop the partial/empty dump file we created before recording it.
    if let Err(e) = dump_result {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }

    let size = std::fs::metadata(&path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);
    if size == 0 {
        let _ = std::fs::remove_file(&path);
        return Err(ApiError::internal("backup produced no data"));
    }
    let storage_uri = format!("file://{}", path.display());
    let id = arx_db::queries::backups::record(&app.db, service.id, size, &storage_uri).await?;

    info!(
        service = %service.slug,
        path = %path.display(),
        bytes = size,
        "backup created"
    );

    if let Ok(Some(sched)) = arx_db::queries::backups::get_schedule(&app.db, service.id).await {
        let pruned =
            arx_db::queries::backups::prune(&app.db, service.id, sched.retention_count).await?;
        for uri in pruned {
            if let Some(p) = uri.strip_prefix("file://") {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    Ok(BackupReport {
        id,
        size_bytes: size,
        storage_uri,
    })
}

pub struct BackupReport {
    pub id: uuid::Uuid,
    pub size_bytes: i64,
    pub storage_uri: String,
}

pub async fn restore(app: &AppState, service: &Service, backup_uri: &str) -> ApiResult<()> {
    let template = template_for(service)?;
    let container = active_container(app, service).await?;
    let path = PathBuf::from(
        backup_uri
            .strip_prefix("file://")
            .ok_or_else(|| ApiError::bad_request("only file:// supported in v1"))?,
    );
    if !path.exists() {
        return Err(ApiError::not_found());
    }

    let user = decoded_var(app, service, "DATABASE_USER").await;
    let db = decoded_var(app, service, "DATABASE_NAME").await;
    let password = decoded_var(app, service, "DATABASE_PASSWORD").await;

    let mut cmd = Command::new("docker");
    cmd.arg("exec").arg("-i");

    let infile = std::fs::File::open(&path).map_err(|e| ApiError::internal(e.to_string()))?;
    match template {
        DbTemplate::Postgres => {
            let user = user.unwrap_or_else(|| "postgres".to_string());
            let db = db.unwrap_or_else(|| "postgres".to_string());
            if let Some(p) = &password {
                cmd.arg("-e").arg(format!("PGPASSWORD={p}"));
            }
            cmd.arg(&container).arg("psql").arg("-U").arg(user).arg(db);
        }
        DbTemplate::Mysql => {
            // Pass the password via MYSQL_PWD env and the db name as a real
            // argument rather than interpolating into a shell command: this
            // hides the password and prevents shell injection via the db name.
            if let Some(p) = &password {
                cmd.arg("-e").arg(format!("MYSQL_PWD={p}"));
            }
            cmd.arg(&container).arg("mysql").arg("-u").arg("root");
            if let Some(db) = db {
                cmd.arg(db);
            }
        }
        DbTemplate::Mongodb => {
            cmd.arg(&container)
                .arg("mongorestore")
                .arg("--archive")
                .arg("--drop");
        }
        DbTemplate::Redis => {
            let cp = Command::new("docker")
                .arg("cp")
                .arg(&path)
                .arg(format!("{container}:/data/dump.rdb"))
                .status()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            if !cp.success() {
                return Err(ApiError::internal("docker cp failed"));
            }
            let restart = Command::new("docker")
                .args(["restart", &container])
                .status()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            if !restart.success() {
                return Err(ApiError::internal("docker restart failed"));
            }
            return Ok(());
        }
    }
    cmd.stdin(Stdio::from(infile));
    let status = cmd
        .status()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if !status.success() {
        return Err(ApiError::internal(format!(
            "restore failed exit {}",
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

fn template_for(service: &Service) -> ApiResult<DbTemplate> {
    match &service.source {
        ServiceSource::DbTemplate { template, .. } => Ok(*template),
        _ => Err(ApiError::bad_request("not a database service")),
    }
}

async fn active_container(app: &AppState, service: &Service) -> ApiResult<String> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT container_id FROM deployments
         WHERE service_id = ? AND status = 'live' AND container_id IS NOT NULL
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(service.id.as_uuid().to_string())
    .fetch_optional(&app.db)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let Some(row) = row else {
        return Err(ApiError::bad_request("service has no live deployment"));
    };
    let id: Option<String> = row.try_get("container_id").ok();
    id.ok_or_else(|| ApiError::internal("missing container_id"))
}

async fn decoded_var(app: &AppState, service: &Service, key: &str) -> Option<String> {
    let envs = arx_db::queries::environments::list_in_project(&app.db, service.project_id)
        .await
        .ok()?;
    for e in envs {
        let injected =
            arx_db::queries::variables::for_injection(&app.db, &app.master_key, service.id, e.id)
                .await
                .ok()?;
        if let Some((_, v)) = injected.iter().find(|(k, _)| k == key) {
            return Some(v.clone());
        }
    }
    None
}

pub fn spawn_scheduler(app: AppState) {
    use std::time::Duration;
    crate::supervisor::spawn_supervised("backups", move || {
        let app = app.clone();
        async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(60));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                if let Err(e) = scheduler_tick(&app).await {
                    warn!(error = %e, "backup scheduler tick failed");
                }
            }
        }
    });
}

async fn scheduler_tick(app: &AppState) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let schedules = arx_db::queries::backups::list_all_enabled(&app.db).await?;
    for s in schedules {
        let last = arx_db::queries::backups::list_for_service(&app.db, s.service_id, 1).await?;
        let due = match last.first() {
            Some(b) => {
                (chrono::Utc::now() - b.created_at)
                    > chrono::Duration::try_hours(24).unwrap_or_default()
            }
            None => true,
        };
        if !due {
            continue;
        }
        let service = match arx_db::queries::services::get_by_id(&app.db, s.service_id).await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "scheduler: service lookup");
                continue;
            }
        };
        match backup_now(app, &service).await {
            Ok(r) => info!(
                service = %service.slug,
                bytes = r.size_bytes,
                "scheduled backup created"
            ),
            Err(e) => warn!(error = ?e, "scheduled backup failed"),
        }
    }
    Ok(())
}

pub async fn list_records(
    app: &AppState,
    service_id: ServiceId,
) -> ApiResult<Vec<arx_db::queries::backups::BackupRecord>> {
    Ok(arx_db::queries::backups::list_for_service(&app.db, service_id, 100).await?)
}
