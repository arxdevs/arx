use crate::auth::Auth;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::web;
use arx_db::queries::auth::CreatedSession;
use arx_db::queries::github as gh_q;
use arx_github::verify_signature;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::header::HeaderMap;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;

const WEB_SESSION_DAYS: i64 = 30;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/setup/github-app/status", get(app_status))
        .route("/v1/webhooks/github", post(webhook))
        .route("/v1/auth/github/oauth-info", get(oauth_info))
        .route("/v1/auth/github/exchange", post(oauth_exchange))
        .route("/v1/auth/github/device", post(device_exchange))
        .route("/v1/auth/github/login", get(web_login))
        .route("/v1/auth/github/callback", get(web_callback))
}

#[derive(serde::Serialize)]
struct OAuthInfo {
    client_id: String,
}

async fn oauth_info(State(app): State<AppState>) -> ApiResult<Json<OAuthInfo>> {
    let creds = arx_db::queries::github::get_app(&app.db, &app.master_key)
        .await?
        .ok_or_else(|| ApiError::bad_request("github app not configured"))?;
    Ok(Json(OAuthInfo {
        client_id: creds.client_id,
    }))
}

#[derive(serde::Deserialize)]
struct ExchangeReq {
    code: String,
}

#[derive(serde::Serialize)]
struct ExchangeResp {
    user: crate::api::UserResp,
    session_token: String,
}

async fn oauth_exchange(
    State(app): State<AppState>,
    Json(req): Json<ExchangeReq>,
) -> ApiResult<Json<ExchangeResp>> {
    let creds = arx_db::queries::github::get_app(&app.db, &app.master_key)
        .await?
        .ok_or_else(|| ApiError::bad_request("github app not configured"))?;

    let http = reqwest::Client::builder()
        .user_agent("arx")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let token_resp: serde_json::Value = http
        .post("https://github.com/login/oauth/access_token")
        .header("accept", "application/json")
        .form(&[
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("code", req.code.as_str()),
        ])
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("github token exchange: {e}")))?
        .json()
        .await
        .map_err(|e| ApiError::internal(format!("decode github token resp: {e}")))?;

    let access_token = token_resp["access_token"].as_str().ok_or_else(|| {
        let err = token_resp["error_description"]
            .as_str()
            .or_else(|| token_resp["error"].as_str())
            .unwrap_or("missing access_token");
        ApiError::bad_request(format!("github oauth: {err}"))
    })?;

    finalize_oauth(&app, &http, access_token).await
}

#[derive(serde::Deserialize)]
struct DeviceExchangeReq {
    access_token: String,
}

async fn device_exchange(
    State(app): State<AppState>,
    Json(req): Json<DeviceExchangeReq>,
) -> ApiResult<Json<ExchangeResp>> {
    let http = reqwest::Client::builder()
        .user_agent("arx")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    finalize_oauth(&app, &http, &req.access_token).await
}

async fn finalize_oauth(
    app: &AppState,
    http: &reqwest::Client,
    access_token: &str,
) -> ApiResult<Json<ExchangeResp>> {
    let (_, display_name, github_login, session) =
        finalize_oauth_session(app, http, access_token, None).await?;
    Ok(Json(ExchangeResp {
        user: crate::api::UserResp {
            id: session.user_id.as_uuid().to_string(),
            display_name,
            github_login: Some(github_login),
        },
        session_token: session.token_plaintext,
    }))
}

async fn finalize_oauth_session(
    app: &AppState,
    http: &reqwest::Client,
    access_token: &str,
    expires_at: Option<chrono::DateTime<Utc>>,
) -> ApiResult<(arx_core::ids::UserId, String, String, CreatedSession)> {
    let me: serde_json::Value = http
        .get("https://api.github.com/user")
        .header("authorization", format!("Bearer {access_token}"))
        .header("accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("github /user: {e}")))?
        .error_for_status()
        .map_err(|e| ApiError::unauthorized_with(format!("github /user: {e}")))?
        .json()
        .await
        .map_err(|e| ApiError::internal(format!("decode github /user: {e}")))?;

    let github_user_id = me["id"]
        .as_i64()
        .ok_or_else(|| ApiError::internal("github user.id missing"))?;
    let github_login = me["login"]
        .as_str()
        .ok_or_else(|| ApiError::internal("github user.login missing"))?
        .to_string();
    let display_name = me["name"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| github_login.clone());
    let avatar_url = me["avatar_url"].as_str().map(|s| s.to_string());

    let (user_id, _created) = arx_db::queries::auth::upsert_github_user(
        &app.db,
        github_user_id,
        &github_login,
        &display_name,
        avatar_url.as_deref(),
    )
    .await?;

    let label = if expires_at.is_some() { "web" } else { "oauth" };
    let session =
        arx_db::queries::auth::issue_session_with_expiry(&app.db, user_id, Some(label), expires_at)
            .await?;

    Ok((user_id, display_name, github_login, session))
}

fn random_state() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

fn login_rate_limit(addr: SocketAddr) -> ApiResult<()> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    static LAST: OnceLock<Mutex<HashMap<std::net::IpAddr, Instant>>> = OnceLock::new();
    const MIN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
    let map = LAST.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let ip = addr.ip();
    if let Some(prev) = guard.get(&ip) {
        if now.duration_since(*prev) < MIN_INTERVAL {
            return Err(ApiError::bad_request("rate limited; try again shortly"));
        }
    }
    guard.insert(ip, now);
    Ok(())
}

async fn web_login(
    State(app): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> ApiResult<Response> {
    login_rate_limit(addr)?;

    let creds = gh_q::get_app(&app.db, &app.master_key)
        .await?
        .ok_or_else(|| ApiError::bad_request("github app not configured"))?;

    let settings = arx_db::queries::settings::get(&app.db).await?;
    let base = settings
        .admin_domain
        .map(|d| format!("https://{d}"))
        .ok_or_else(|| ApiError::bad_request("admin domain not configured"))?;
    let redirect_uri = format!("{base}/v1/auth/github/callback");

    let state = random_state();
    app.remember_oauth_state(state.clone());

    let scope = "read:user user:email";
    let authorize_url = format!(
        "https://github.com/login/oauth/authorize?client_id={cid}&redirect_uri={ru}&state={st}&scope={sc}",
        cid = creds.client_id,
        ru = urlencode(&redirect_uri),
        st = urlencode(&state),
        sc = urlencode(scope),
    );

    let secure = web::cookie_is_secure(&app).await;
    let state_cookie = web::set_cookie("arx_oauth_state", &state, 300, secure);

    Ok((
        [(header::SET_COOKIE, state_cookie)],
        Redirect::to(&authorize_url),
    )
        .into_response())
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn web_callback(
    State(app): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Response {
    if login_rate_limit(addr).is_err() {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response();
    }

    if q.error.is_some() {
        return (StatusCode::BAD_REQUEST, "github oauth was denied").into_response();
    }

    let (Some(code), Some(state)) = (q.code, q.state) else {
        return (StatusCode::BAD_REQUEST, "missing code or state").into_response();
    };

    let cookie_state = cookie_value(&headers, "arx_oauth_state");
    let cookie_ok = cookie_state
        .as_deref()
        .map(|c| constant_time_eq(c.as_bytes(), state.as_bytes()))
        .unwrap_or(false);

    if !cookie_ok || !app.take_oauth_state(&state) {
        return (StatusCode::BAD_REQUEST, "invalid or expired oauth state").into_response();
    }

    let creds = match gh_q::get_app(&app.db, &app.master_key).await {
        Ok(Some(c)) => c,
        _ => return (StatusCode::BAD_REQUEST, "github app not configured").into_response(),
    };

    let http = match reqwest::Client::builder()
        .user_agent("arx")
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let token_resp: serde_json::Value = match http
        .post("https://github.com/login/oauth/access_token")
        .header("accept", "application/json")
        .form(&[
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("code", code.as_str()),
        ])
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        Ok(r) => match r.json().await {
            Ok(v) => v,
            Err(_) => return StatusCode::BAD_GATEWAY.into_response(),
        },
        Err(_) => return StatusCode::BAD_GATEWAY.into_response(),
    };

    let Some(access_token) = token_resp["access_token"].as_str() else {
        return (StatusCode::BAD_REQUEST, "github oauth exchange failed").into_response();
    };

    let expires_at = Utc::now() + Duration::days(WEB_SESSION_DAYS);
    let session = match finalize_oauth_session(&app, &http, access_token, Some(expires_at)).await {
        Ok((_, _, _, s)) => s,
        Err(e) => return (e.0, e.2).into_response(),
    };

    let secure = web::cookie_is_secure(&app).await;
    let session_cookie = web::session_cookie(
        &session.token_plaintext,
        WEB_SESSION_DAYS * 24 * 60 * 60,
        secure,
    );
    let clear_state = web::clear_cookie("arx_oauth_state", secure);

    web::redirect_to_root(vec![session_cookie, clear_state])
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k.trim() == name).then(|| v.trim().to_string())
    })
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}

async fn app_status(
    Auth(_): Auth,
    State(app): State<AppState>,
) -> ApiResult<Json<serde_json::Value>> {
    let creds = gh_q::get_app(&app.db, &app.master_key).await?;
    Ok(Json(match creds {
        Some(c) => json!({
            "configured": true,
            "app_id": c.app_id,
            "slug": c.slug,
            "name": c.name,
            "html_url": c.html_url,
        }),
        None => json!({"configured": false}),
    }))
}

async fn webhook(
    State(app): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    let creds = gh_q::get_app(&app.db, &app.master_key)
        .await?
        .ok_or_else(|| ApiError::bad_request("github app not configured"))?;

    let sig = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok());
    verify_signature(creds.webhook_secret.as_bytes(), sig, &body)
        .map_err(|e| ApiError::bad_request(format!("signature: {e}")))?;

    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    let delivery = headers
        .get("x-github-delivery")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let payload: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| ApiError::bad_request(e.to_string()))?;

    let id = uuid::Uuid::now_v7();
    let now = chrono::Utc::now().to_rfc3339();
    gh_q::record_event(
        &app.db,
        &id.to_string(),
        &event,
        delivery.as_deref(),
        &String::from_utf8_lossy(&body),
        &now,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    match event.as_str() {
        "ping" => {}
        "push" => {
            handle_push(app.clone(), payload.clone(), id.to_string()).await;
        }
        // Installation membership changed. The webhook is only a fast path; we
        // re-derive the full installation set from the API (the truth source)
        // rather than trusting the payload, so a missed delivery self-heals.
        "installation" | "installation_repositories" => {
            handle_installation_change(app.clone(), id.to_string()).await;
        }
        other => {
            tracing::info!(event = other, "unhandled event");
        }
    }

    Ok(Json(json!({"ok": true})))
}

async fn handle_installation_change(app: AppState, event_id: String) {
    match crate::github_sync::reconcile_installations(&app).await {
        Ok(report) => {
            tracing::info!(
                installations = report.installations,
                repos = report.repos,
                "reconciled github installations from webhook"
            );
            mark_processed(&app, &event_id, None).await;
        }
        Err(e) => {
            tracing::error!(error = %e.2, "installation reconcile from webhook failed");
            mark_processed(&app, &event_id, Some(&e.2)).await;
        }
    }
}

async fn handle_push(app: AppState, payload: serde_json::Value, event_id: String) {
    let repo = payload
        .get("repository")
        .and_then(|r| r.get("full_name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let ref_str = payload
        .get("ref")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let (Some(repo), Some(ref_full)) = (repo, ref_str) else {
        tracing::warn!(?payload, "push event missing repository.full_name or ref");
        mark_processed(&app, &event_id, Some("missing repo/ref")).await;
        return;
    };

    let branch = ref_full
        .strip_prefix("refs/heads/")
        .unwrap_or(&ref_full)
        .to_string();

    let targets = match arx_db::queries::services::find_git_targets(&app.db, &repo, &branch).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, "find_git_targets");
            mark_processed(&app, &event_id, Some(&format!("find: {e}"))).await;
            return;
        }
    };

    if targets.is_empty() {
        tracing::info!(repo, branch, "push received but no matching services");
        mark_processed(&app, &event_id, None).await;
        return;
    }

    let changed_paths = changed_paths_from_payload(&payload);

    for t in targets {
        if !path_matches(
            &changed_paths,
            t.root_directory.as_deref(),
            t.watch_paths.as_deref(),
        ) {
            tracing::info!(
                repo,
                branch,
                service = %t.service_slug,
                "push: service skipped (no path match)"
            );
            continue;
        }
        crate::deploy_queue::enqueue(app.clone(), t);
    }

    mark_processed(&app, &event_id, None).await;
}

/// Returns `None` when the payload has no usable `commits` array — callers
/// should treat that as "match everything" (safe fallback for tag pushes,
/// branch creation, oversized push payloads, etc.).
fn changed_paths_from_payload(payload: &serde_json::Value) -> Option<Vec<String>> {
    let arr = payload.get("commits").and_then(|v| v.as_array())?;
    if arr.is_empty() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for commit in arr {
        for field in ["added", "modified", "removed"] {
            if let Some(list) = commit.get(field).and_then(|v| v.as_array()) {
                for p in list {
                    if let Some(s) = p.as_str() {
                        out.push(s.to_string());
                    }
                }
            }
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn path_matches(
    changed: &Option<Vec<String>>,
    root_directory: Option<&str>,
    watch_paths: Option<&[String]>,
) -> bool {
    // Safe fallback: no per-commit file list available → match everything.
    let Some(changed) = changed else {
        return true;
    };

    let mut builder = globset::GlobSetBuilder::new();
    let mut added_any = false;

    let push_pattern = |b: &mut globset::GlobSetBuilder, added: &mut bool, pat: &str| {
        if let Ok(g) = globset::GlobBuilder::new(pat)
            .literal_separator(false)
            .build()
        {
            b.add(g);
            *added = true;
        }
    };

    match watch_paths {
        Some(user_patterns) if !user_patterns.is_empty() => {
            for p in user_patterns {
                push_pattern(&mut builder, &mut added_any, p);
            }
        }
        _ => {
            if let Some(rd) = root_directory.and_then(|s| {
                let trimmed = s.trim_matches('/');
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            }) {
                push_pattern(&mut builder, &mut added_any, &format!("{rd}/**"));
                for marker in [
                    "**/pnpm-lock.yaml",
                    "**/package-lock.json",
                    "**/yarn.lock",
                    "**/bun.lockb",
                    "**/bun.lock",
                    "**/turbo.json",
                    "**/pnpm-workspace.yaml",
                    "**/pnpm-workspace.yml",
                    "package.json",
                ] {
                    push_pattern(&mut builder, &mut added_any, marker);
                }
            } else {
                return true;
            }
        }
    }

    if !added_any {
        return true;
    }

    let set = match builder.build() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "watch glob build failed; defaulting to match");
            return true;
        }
    };

    changed.iter().any(|p| set.is_match(p))
}

pub(crate) async fn run_deploy_target(
    app: &AppState,
    t: &arx_db::queries::services::GitTarget,
) -> arx_core::Result<()> {
    use arx_db::queries::{environments, projects, services, workspaces};

    let workspace = workspaces::get_by_id(&app.db, t.workspace_id).await?;
    let project = projects::get_by_id(&app.db, t.project_id).await?;
    let service = services::get_by_id(&app.db, t.service_id).await?;
    let environment = environments::get_by_id(&app.db, t.environment_id).await?;

    match crate::deploy::deploy(app, &workspace, &project, &service, &environment).await {
        Ok(d) => {
            tracing::info!(
                deployment_id = %d.id.as_uuid(),
                workspace = %workspace.slug,
                project = %project.slug,
                service = %service.slug,
                env = %environment.slug,
                "auto-deploy live"
            );
            Ok(())
        }
        Err(e) => Err(arx_core::Error::Internal(format!("deploy: {:?}", e.2))),
    }
}

async fn mark_processed(app: &AppState, id: &str, error: Option<&str>) {
    let now = chrono::Utc::now().to_rfc3339();
    let _ = sqlx::query(
        "UPDATE webhook_events SET processed = 1, processed_at = ?, error = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(error)
    .bind(id)
    .execute(&app.db)
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn changed_paths_extracts_added_modified_removed() {
        let p = json!({
            "commits": [
                {"added": ["a.txt"], "modified": ["b.txt"], "removed": ["c.txt"]},
                {"added": [], "modified": ["apps/web/x.ts"], "removed": null},
            ]
        });
        let mut got = changed_paths_from_payload(&p).unwrap();
        got.sort();
        assert_eq!(got, vec!["a.txt", "apps/web/x.ts", "b.txt", "c.txt"]);
    }

    #[test]
    fn changed_paths_returns_none_when_commits_missing() {
        assert!(changed_paths_from_payload(&json!({})).is_none());
        assert!(changed_paths_from_payload(&json!({"commits": []})).is_none());
    }

    #[test]
    fn matches_root_directory_glob() {
        let changed = Some(vec!["apps/web/src/index.ts".to_string()]);
        assert!(path_matches(&changed, Some("apps/web"), None));
        assert!(!path_matches(&changed, Some("apps/api"), None));
    }

    #[test]
    fn matches_lockfile_default_in_monorepo() {
        let changed = Some(vec!["pnpm-lock.yaml".to_string()]);
        assert!(path_matches(&changed, Some("apps/web"), None));
        assert!(path_matches(&changed, Some("apps/api"), None));
    }

    #[test]
    fn user_patterns_take_priority_over_defaults() {
        // Even though the lockfile would match default patterns, user-supplied
        // watch_paths should narrow the match.
        let changed = Some(vec!["pnpm-lock.yaml".to_string()]);
        let user = vec!["apps/web/**".to_string()];
        assert!(!path_matches(&changed, Some("apps/web"), Some(&user)));
    }

    #[test]
    fn no_root_no_watch_matches_everything() {
        let changed = Some(vec!["random/file.txt".to_string()]);
        assert!(path_matches(&changed, None, None));
    }

    #[test]
    fn no_commits_safe_fallback_matches() {
        assert!(path_matches(&None, Some("apps/web"), None));
    }

    #[test]
    fn empty_watch_paths_falls_back_to_default() {
        let changed = Some(vec!["apps/web/x.ts".to_string()]);
        let empty: Vec<String> = vec![];
        assert!(path_matches(&changed, Some("apps/web"), Some(&empty)));
    }
}
