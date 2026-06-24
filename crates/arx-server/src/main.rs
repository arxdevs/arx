mod api;
mod audit_pruner;
mod auth;
mod backups;
mod cascade;
mod cert_poll;
mod db_template;
mod deploy;
mod deploy_queue;
mod dns_verify;
mod error;
mod github_routes;
mod github_sync;
mod setup;
mod state;
mod supervisor;
mod var_resolve;
mod volumes;
mod webhooks;

use anyhow::Context;
use arx_core::config::Config;
use arx_db::crypto::MasterKey;
use arx_docker::{ContainerEngine, DockerEngine};
use arx_traefik::TraefikWriter;
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

const SHUTDOWN_GRACE_SECS: u64 = 30;

#[derive(Debug, Parser)]
#[command(name = "arx-server", version, about = "arx daemon")]
struct Cli {
    #[arg(
        short,
        long,
        env = "ARX_CONFIG",
        default_value = "/etc/arx/config.toml"
    )]
    config: PathBuf,

    #[arg(long, env = "ARX_LISTEN")]
    listen: Option<SocketAddr>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cli = Cli::parse();
    let mut cfg = if cli.config.exists() {
        Config::load(&cli.config).context("loading config")?
    } else {
        tracing::warn!(
            path = %cli.config.display(),
            "config file not found; using defaults"
        );
        Config {
            server: Default::default(),
            paths: Default::default(),
            traefik: Default::default(),
        }
    };
    if let Some(addr) = cli.listen {
        cfg.server.listen = addr;
    }

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "arx-server starting");

    std::fs::create_dir_all(&cfg.paths.data_dir)?;

    let db = arx_db::connect(&cfg.paths.db_path)
        .await
        .context("connecting to database")?;
    tracing::info!(path = %cfg.paths.db_path.display(), "database ready");

    let master_key =
        MasterKey::load_or_create(&cfg.paths.master_key_path).context("loading master key")?;

    let docker =
        DockerEngine::connect_local().context("connecting to docker (is the daemon running?)")?;

    let traefik = TraefikWriter::new(
        cfg.traefik.dynamic_config_path.clone(),
        cfg.traefik.admin_api_url.clone(),
    );

    if let Err(e) = traefik.write_routes(&[]) {
        tracing::warn!(error = %e, "could not pre-write traefik dynamic config");
    }

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("building shared http client")?;

    let state = state::AppState {
        db: db.clone(),
        master_key: Arc::new(master_key),
        traefik,
        docker: Arc::new(docker),
        config: Arc::new(cfg.clone()),
        deploy_locks: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        traefik_lock: Arc::new(tokio::sync::Mutex::new(())),
        deploy_queue: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        in_flight_deploys: Arc::new(AtomicUsize::new(0)),
        http,
    };

    cleanup_interrupted_deployments(&state).await;

    cert_poll::spawn(state.clone());
    backups::spawn_scheduler(state.clone());
    audit_pruner::spawn(state.clone());
    webhooks::spawn_worker(state.clone());
    webhooks::spawn_pruner(state.clone());

    let app = api::router(state.clone()).layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(cfg.server.listen)
        .await
        .with_context(|| format!("binding {}", cfg.server.listen))?;
    tracing::info!(addr = %cfg.server.listen, "HTTP listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    await_in_flight_deploys(&state.in_flight_deploys, SHUTDOWN_GRACE_SECS).await;
    cleanup_interrupted_deployments(&state).await;

    drop(db);
    Ok(())
}

async fn cleanup_interrupted_deployments(state: &state::AppState) {
    match arx_db::queries::deployments::mark_interrupted_as_failed(&state.db).await {
        Ok(items) if !items.is_empty() => {
            tracing::warn!(
                count = items.len(),
                "marked interrupted deployments as failed; cleaning up containers"
            );
            for (_, container_id) in items {
                if let Some(id) = container_id {
                    let handle = arx_docker::ContainerHandle(id);
                    if let Err(e) = state.docker.stop_and_remove(&handle).await {
                        tracing::debug!(error = %e, "container cleanup failed (may already be gone)");
                    }
                }
            }
        }
        Ok(_) => {}
        Err(e) => tracing::error!(error = %e, "failed to mark interrupted deployments"),
    }
}

async fn await_in_flight_deploys(counter: &Arc<AtomicUsize>, grace_secs: u64) {
    let deadline = std::time::Instant::now() + Duration::from_secs(grace_secs);
    loop {
        let n = counter.load(Ordering::SeqCst);
        if n == 0 {
            return;
        }
        if std::time::Instant::now() >= deadline {
            tracing::warn!(
                in_flight = n,
                "shutdown grace expired with deploys still running"
            );
            return;
        }
        tracing::info!(in_flight = n, "awaiting in-flight deploys");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,arx=debug,sqlx=warn,tower_http=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .json()
        .flatten_event(true)
        .init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
