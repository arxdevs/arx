//! Thin GitHub REST client for the App-authenticated endpoints arx needs:
//! listing installations, minting installation access tokens, listing the
//! repositories an installation can reach, and updating the App's webhook URL.
//!
//! Authentication is the caller's responsibility: pass an App JWT (from
//! [`crate::app_auth::app_jwt`]) for `/app/*` calls, or an installation token
//! for `/installation/*` calls.

use arx_core::Error;
use serde::Deserialize;

const API_BASE: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
const PER_PAGE: u32 = 100;

#[derive(Debug, Clone, Deserialize)]
pub struct Installation {
    pub id: i64,
    pub account: Option<Account>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    pub login: String,
    #[serde(rename = "type")]
    pub account_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstallationToken {
    pub token: String,
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
struct RepositoriesPage {
    repositories: Vec<Repository>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Repository {
    pub full_name: String,
}

fn map_send(context: &str, e: reqwest::Error) -> Error {
    Error::Internal(format!("github {context}: {e}"))
}

async fn ensure_ok(context: &str, res: reqwest::Response) -> Result<reqwest::Response, Error> {
    let status = res.status();
    if status.is_success() {
        return Ok(res);
    }
    // Body often carries GitHub's machine-readable error; include it (it never
    // contains the bearer token).
    let body = res.text().await.unwrap_or_default();
    Err(Error::Internal(format!(
        "github {context} returned {status}: {body}"
    )))
}

/// Lists every installation of the App, following pagination.
pub async fn list_installations(
    http: &reqwest::Client,
    app_jwt: &str,
) -> Result<Vec<Installation>, Error> {
    let mut out = Vec::new();
    let mut page = 1u32;
    loop {
        let res = http
            .get(format!("{API_BASE}/app/installations"))
            .query(&[("per_page", PER_PAGE), ("page", page)])
            .bearer_auth(app_jwt)
            .header("accept", "application/vnd.github+json")
            .header("user-agent", "arx")
            .header("x-github-api-version", API_VERSION)
            .send()
            .await
            .map_err(|e| map_send("list installations", e))?;
        let res = ensure_ok("list installations", res).await?;
        let batch: Vec<Installation> = res
            .json()
            .await
            .map_err(|e| map_send("decode installations", e))?;
        let count = batch.len();
        out.extend(batch);
        if count < PER_PAGE as usize {
            break;
        }
        page += 1;
    }
    Ok(out)
}

/// Mints a short-lived installation access token for cloning private repos and
/// reading installation-scoped data.
pub async fn installation_token(
    http: &reqwest::Client,
    app_jwt: &str,
    installation_id: i64,
) -> Result<InstallationToken, Error> {
    let res = http
        .post(format!(
            "{API_BASE}/app/installations/{installation_id}/access_tokens"
        ))
        .bearer_auth(app_jwt)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", "arx")
        .header("x-github-api-version", API_VERSION)
        .send()
        .await
        .map_err(|e| map_send("mint installation token", e))?;
    let res = ensure_ok("mint installation token", res).await?;
    res.json()
        .await
        .map_err(|e| map_send("decode installation token", e))
}

/// Lists every repository an installation can reach (using an installation
/// token, not the App JWT), following pagination.
pub async fn installation_repositories(
    http: &reqwest::Client,
    installation_token: &str,
) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    let mut page = 1u32;
    loop {
        let res = http
            .get(format!("{API_BASE}/installation/repositories"))
            .query(&[("per_page", PER_PAGE), ("page", page)])
            .bearer_auth(installation_token)
            .header("accept", "application/vnd.github+json")
            .header("user-agent", "arx")
            .header("x-github-api-version", API_VERSION)
            .send()
            .await
            .map_err(|e| map_send("list installation repositories", e))?;
        let res = ensure_ok("list installation repositories", res).await?;
        let body: RepositoriesPage = res
            .json()
            .await
            .map_err(|e| map_send("decode installation repositories", e))?;
        let count = body.repositories.len();
        out.extend(body.repositories.into_iter().map(|r| r.full_name));
        if count < PER_PAGE as usize {
            break;
        }
        page += 1;
    }
    Ok(out)
}

/// Updates the App's webhook delivery URL via `PATCH /app/hook/config`.
///
/// Note: this REST endpoint only updates the **webhook** URL. A GitHub App's
/// `callback_urls` and homepage `url` are not mutable via the REST API; they
/// can only be changed in the App settings UI or by re-running the manifest
/// flow. Callers that need those updated must surface that limitation.
pub async fn update_hook_url(
    http: &reqwest::Client,
    app_jwt: &str,
    webhook_url: &str,
) -> Result<(), Error> {
    let res = http
        .patch(format!("{API_BASE}/app/hook/config"))
        .bearer_auth(app_jwt)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", "arx")
        .header("x-github-api-version", API_VERSION)
        .json(&serde_json::json!({ "url": webhook_url }))
        .send()
        .await
        .map_err(|e| map_send("update hook config", e))?;
    ensure_ok("update hook config", res).await?;
    Ok(())
}
