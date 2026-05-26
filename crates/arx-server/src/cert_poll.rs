use crate::state::AppState;
use arx_core::model::CertStatus;
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, warn};

pub fn spawn(app: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(30));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Err(e) = tick(&app).await {
                debug!(error = %e, "cert poll tick failed");
            }
        }
    });
}

#[derive(Deserialize)]
struct RouterInfo {
    rule: Option<String>,
    status: Option<String>,
    #[serde(default)]
    tls: Option<serde_json::Value>,
}

async fn tick(app: &AppState) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!(
        "{}/api/http/routers",
        app.config.traefik.admin_api_url.trim_end_matches('/')
    );
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let routers: Vec<RouterInfo> = match http.get(&url).send().await {
        Ok(r) if r.status().is_success() => r.json().await?,
        Ok(r) => {
            debug!(status = %r.status(), "traefik api non-success");
            return Ok(());
        }
        Err(e) => {
            debug!(error = %e, "traefik api unreachable");
            return Ok(());
        }
    };

    let domains = arx_db::queries::domains::list_all_active(&app.db).await?;
    for d in domains {
        let hostname = d.hostname.clone();
        let matched = routers.iter().find(|r| {
            r.rule
                .as_deref()
                .map(|rule| rule.contains(&format!("`{hostname}`")))
                .unwrap_or(false)
        });
        let new_status = match matched {
            Some(r) if r.status.as_deref() == Some("enabled") && r.tls.is_some() => {
                CertStatus::Issued
            }
            Some(r) if r.status.as_deref() == Some("disabled") => CertStatus::Failed,
            _ => CertStatus::Pending,
        };
        if new_status != d.cert_status {
            if let Err(e) =
                sqlx::query("UPDATE domains SET cert_status = ?, verified = ? WHERE id = ?")
                    .bind(new_status.as_str())
                    .bind(matches!(new_status, CertStatus::Issued) as i64)
                    .bind(d.id.as_uuid().to_string())
                    .execute(&app.db)
                    .await
            {
                warn!(error = %e, "update cert_status failed");
            } else if matches!(new_status, CertStatus::Failed) {
                let _ = arx_db::queries::audit::write(
                    &app.db,
                    None,
                    "cert.failed",
                    &format!("domain:{}", d.hostname),
                    serde_json::json!({"previous": d.cert_status.as_str()}),
                )
                .await;
            }
        }
    }

    Ok(())
}
