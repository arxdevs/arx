use crate::state::AppState;
use crate::supervisor::spawn_supervised;
use std::time::Duration;
use tracing::{debug, warn};

const RETAIN_DAYS: i64 = 90;
const TICK: Duration = Duration::from_secs(60 * 60 * 24);

pub fn spawn(app: AppState) {
    spawn_supervised("audit_pruner", move || {
        let app = app.clone();
        async move {
            let mut ticker = tokio::time::interval(TICK);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                match arx_db::queries::audit::prune(
                    &app.db,
                    chrono::Duration::try_days(RETAIN_DAYS).unwrap_or_default(),
                )
                .await
                {
                    Ok(n) if n > 0 => debug!(deleted = n, "audit_logs pruned"),
                    Ok(_) => {}
                    Err(e) => warn!(error = %e, "audit prune failed"),
                }
            }
        }
    });
}
