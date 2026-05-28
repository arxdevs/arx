use arx_core::config::Config;
use arx_core::ids::{EnvironmentId, ServiceId};
use arx_db::Db;
use arx_db::crypto::MasterKey;
use arx_docker::DockerEngine;
use arx_traefik::TraefikWriter;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

pub type DeployLockMap = Arc<Mutex<HashMap<(ServiceId, EnvironmentId), Arc<AsyncMutex<()>>>>>;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub master_key: Arc<MasterKey>,
    pub traefik: TraefikWriter,
    pub docker: Arc<DockerEngine>,
    pub config: Arc<Config>,
    pub deploy_locks: DeployLockMap,
    pub traefik_lock: Arc<AsyncMutex<()>>,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn deploy_lock(
        &self,
        service_id: ServiceId,
        environment_id: EnvironmentId,
    ) -> Arc<AsyncMutex<()>> {
        let mut map = self.deploy_locks.lock().unwrap_or_else(|e| e.into_inner());
        map.entry((service_id, environment_id))
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }
}
