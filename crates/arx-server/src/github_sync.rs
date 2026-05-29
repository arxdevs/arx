//! Pull-based reconciliation of GitHub App installations.
//!
//! GitHub is the truth source: webhooks are a best-effort fast path that can be
//! missed (server down, dropped delivery, GitHub outage), so this routine
//! re-derives local state from the API. It lists every installation of the App
//! and the repositories each can reach, writes them into the DB, and removes
//! installations that no longer exist.

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use arx_db::queries::github as gh_q;

#[derive(Debug, serde::Serialize)]
pub struct SyncReport {
    pub installations: usize,
    pub repos: usize,
}

/// Reconciles installations and their repositories from the GitHub API.
pub async fn reconcile_installations(app: &AppState) -> ApiResult<SyncReport> {
    let creds = gh_q::get_app(&app.db, &app.master_key)
        .await?
        .ok_or_else(|| ApiError::bad_request("github app not configured"))?;
    let jwt = arx_github::app_jwt(creds.app_id, &creds.private_key_pem)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let installations = arx_github::api::list_installations(&app.http, &jwt)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let now = chrono::Utc::now().to_rfc3339();
    let mut total_repos = 0usize;
    let mut keep = Vec::with_capacity(installations.len());

    for inst in &installations {
        let (login, kind) = inst
            .account
            .as_ref()
            .map(|a| (a.login.as_str(), a.account_type.as_str()))
            .unwrap_or(("", "Unknown"));
        gh_q::upsert_installation(&app.db, inst.id, login, kind, &now).await?;

        let token = arx_github::api::installation_token(&app.http, &jwt, inst.id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let repos = arx_github::api::installation_repositories(&app.http, &token.token)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        total_repos += repos.len();
        gh_q::set_installation_repos(&app.db, inst.id, &repos).await?;
        keep.push(inst.id);
    }

    let removed = gh_q::delete_installations_not_in(&app.db, &keep).await?;
    if removed > 0 {
        tracing::info!(removed, "pruned stale github installations during sync");
    }

    Ok(SyncReport {
        installations: installations.len(),
        repos: total_repos,
    })
}
