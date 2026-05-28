use crate::state::AppState;
use arx_core::ids::{EnvironmentId, ServiceId};
use arx_db::queries::services::GitTarget;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

pub type CoalesceKey = (ServiceId, EnvironmentId);

#[derive(Default)]
pub struct Slot {
    pub next: Option<GitTarget>,
}

pub type DeployQueue = Arc<Mutex<HashMap<CoalesceKey, Slot>>>;

pub fn enqueue(app: AppState, target: GitTarget) {
    let key = (target.service_id, target.environment_id);
    let mut map = app.deploy_queue.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = map.get_mut(&key) {
        slot.next = Some(target);
        tracing::info!(
            service_id = %key.0.as_uuid(),
            env_id = %key.1.as_uuid(),
            "deploy already in flight, coalescing latest target"
        );
        return;
    }
    map.insert(key, Slot::default());
    drop(map);

    app.in_flight_deploys.fetch_add(1, Ordering::SeqCst);
    tokio::spawn(run_loop(app, key, target));
}

async fn run_loop(app: AppState, key: CoalesceKey, initial: GitTarget) {
    let mut current = initial;
    loop {
        if let Err(e) = super::github_routes::run_deploy_target(&app, &current).await {
            tracing::error!(error = %e, target = ?current, "auto-deploy failed");
        }
        let next = {
            let mut map = app.deploy_queue.lock().unwrap_or_else(|e| e.into_inner());
            match map.get_mut(&key).and_then(|s| s.next.take()) {
                Some(t) => Some(t),
                None => {
                    map.remove(&key);
                    None
                }
            }
        };
        match next {
            Some(t) => current = t,
            None => break,
        }
    }
    app.in_flight_deploys.fetch_sub(1, Ordering::SeqCst);
}
