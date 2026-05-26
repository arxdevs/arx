use crate::auth::Auth;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use arx_db::queries::github as gh_q;
use arx_github::verify_signature;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::header::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/setup/github-app/status", get(app_status))
        .route("/v1/webhooks/github", post(webhook))
        .route("/v1/auth/github/oauth-info", get(oauth_info))
        .route("/v1/auth/github/exchange", post(oauth_exchange))
        .route("/v1/auth/github/device", post(device_exchange))
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

    let session = arx_db::queries::auth::issue_session(&app.db, user_id, Some("oauth")).await?;

    Ok(Json(ExchangeResp {
        user: crate::api::UserResp {
            id: user_id.as_uuid().to_string(),
            display_name,
            github_login: Some(github_login),
        },
        session_token: session.token_plaintext,
    }))
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
    sqlx::query(
        "INSERT INTO webhook_events (id, source, event_type, delivery_id, payload, processed, error, received_at, processed_at)
         VALUES (?, 'github', ?, ?, ?, 0, NULL, ?, NULL)
         ON CONFLICT (source, delivery_id) DO NOTHING",
    )
    .bind(id.to_string())
    .bind(&event)
    .bind(delivery)
    .bind(String::from_utf8_lossy(&body).into_owned())
    .bind(&now)
    .execute(&app.db)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    match event.as_str() {
        "ping" => {}
        "push" => {
            handle_push(app.clone(), payload.clone(), id.to_string()).await;
        }
        other => {
            tracing::info!(event = other, "unhandled event");
        }
    }

    Ok(Json(json!({"ok": true})))
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

    for t in targets {
        let app = app.clone();
        tokio::spawn(async move {
            let res = run_deploy(&app, &t).await;
            if let Err(e) = res {
                tracing::error!(error = %e, target = ?t, "auto-deploy failed");
            }
        });
    }

    mark_processed(&app, &event_id, None).await;
}

async fn run_deploy(
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
