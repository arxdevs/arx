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
    /// The webhook URL pointed at the App, when `--app` reconciliation ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
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
        webhook_url: None,
    })
}

/// Re-points the GitHub App's webhook URL at this server's current admin domain
/// via the REST API. Returns the URL that was set, or `Ok(None)` if no admin
/// domain is configured yet (nothing to point at).
///
/// Only the webhook URL is updatable via REST; a GitHub App's callback and
/// homepage URLs cannot be changed this way (see
/// [`arx_github::api::update_hook_url`]) and must be fixed in the App settings
/// UI or by re-running the manifest flow.
pub async fn update_app_webhook_url(app: &AppState) -> ApiResult<Option<String>> {
    let settings = arx_db::queries::settings::get(&app.db).await?;
    let Some(domain) = settings.admin_domain else {
        return Ok(None);
    };
    let webhook_url = format!("https://{domain}/v1/webhooks/github");

    let creds = gh_q::get_app(&app.db, &app.master_key)
        .await?
        .ok_or_else(|| ApiError::bad_request("github app not configured"))?;
    let jwt = arx_github::app_jwt(creds.app_id, &creds.private_key_pem)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    arx_github::api::update_hook_url(&app.http, &jwt, &webhook_url)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Some(webhook_url))
}

/// Mints a short-lived installation access token for cloning `repo_full_name`,
/// if some installation is known to reach it.
///
/// Returns `Ok(None)` when no installation maps to the repo (e.g. a public repo,
/// or installations not synced yet) so the caller falls back to an
/// unauthenticated clone. Returns `Err` only when an installation *is* mapped
/// but the App is unconfigured or the token mint fails — surfacing that beats a
/// confusing downstream clone error.
pub async fn clone_token_for_repo(
    app: &AppState,
    repo_full_name: &str,
) -> ApiResult<Option<String>> {
    let Some(installation_id) = gh_q::installation_for_repo(&app.db, repo_full_name).await? else {
        return Ok(None);
    };
    let creds = gh_q::get_app(&app.db, &app.master_key)
        .await?
        .ok_or_else(|| ApiError::bad_request("github app not configured"))?;
    let jwt = arx_github::app_jwt(creds.app_id, &creds.private_key_pem)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let token = arx_github::api::installation_token(&app.http, &jwt, installation_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Some(token.token))
}
