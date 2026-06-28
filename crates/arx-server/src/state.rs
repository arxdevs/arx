use arx_core::config::Config;
use arx_core::ids::{EnvironmentId, ServiceId};
use arx_db::Db;
use arx_db::crypto::MasterKey;
use arx_docker::DockerEngine;
use arx_traefik::TraefikWriter;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::Mutex as AsyncMutex;

pub type DeployLockMap = Arc<Mutex<HashMap<(ServiceId, EnvironmentId), Arc<AsyncMutex<()>>>>>;

/// In-memory store of pending OAuth `state` values for the browser login flow.
/// Single-use, short-lived (see [`OAUTH_STATE_TTL`]). Lost on daemon restart,
/// which only fails in-flight logins (the user retries) — acceptable for a
/// single-daemon deployment.
pub type OAuthStateMap = Arc<Mutex<HashMap<String, Instant>>>;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub master_key: Arc<MasterKey>,
    pub traefik: TraefikWriter,
    pub docker: Arc<DockerEngine>,
    pub config: Arc<Config>,
    pub deploy_locks: DeployLockMap,
    pub traefik_lock: Arc<AsyncMutex<()>>,
    pub deploy_queue: crate::deploy_queue::DeployQueue,
    pub in_flight_deploys: Arc<AtomicUsize>,
    pub http: reqwest::Client,
    pub oauth_states: OAuthStateMap,
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

    /// Record a freshly minted OAuth `state` for later single-use validation.
    /// Also opportunistically evicts expired entries so the map stays bounded
    /// even under unauthenticated `GET /v1/auth/github/login` traffic.
    pub fn remember_oauth_state(&self, state: String) {
        let mut map = self.oauth_states.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        map.retain(|_, t| now.duration_since(*t) < OAUTH_STATE_TTL);
        map.insert(state, now);
    }

    /// Consume an OAuth `state`: returns `true` exactly once for a valid,
    /// unexpired value, then removes it (single-use, replay-proof).
    pub fn take_oauth_state(&self, state: &str) -> bool {
        let mut map = self.oauth_states.lock().unwrap_or_else(|e| e.into_inner());
        match map.remove(state) {
            Some(t) => Instant::now().duration_since(t) < OAUTH_STATE_TTL,
            None => false,
        }
    }
}

/// How long a pending OAuth `state` stays valid between `login` and `callback`.
pub const OAUTH_STATE_TTL: std::time::Duration = std::time::Duration::from_secs(300);
