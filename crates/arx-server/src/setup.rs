use crate::api::{UserResp, WorkspaceResp};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use arx_db::queries::{auth as auth_q, github as gh_q, workspaces};
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/setup-status", get(setup_status))
        .route("/v1/setup/github-app/install", post(install_github_app))
        .route(
            "/v1/server/settings",
            get(get_settings).patch(patch_settings),
        )
        .route("/v1/server/cert/retry", post(cert_retry))
        .route("/v1/server/github/sync", post(sync_github))
}

async fn sync_github(
    crate::auth::Auth(_): crate::auth::Auth,
    State(app): State<AppState>,
) -> ApiResult<Json<crate::github_sync::SyncReport>> {
    let report = crate::github_sync::reconcile_installations(&app).await?;
    Ok(Json(report))
}

async fn cert_retry(
    crate::auth::Auth(_): crate::auth::Auth,
    State(app): State<AppState>,
) -> ApiResult<Json<serde_json::Value>> {
    let res =
        sqlx::query("UPDATE domains SET cert_status = 'pending' WHERE cert_status = 'failed'")
            .execute(&app.db)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    crate::deploy::rewrite_traefik(&app).await?;
    Ok(Json(serde_json::json!({
        "reset_count": res.rows_affected(),
    })))
}

#[derive(Serialize)]
struct ServerSettingsResp {
    admin_domain: Option<String>,
    acme_email: Option<String>,
    public_ip: Option<String>,
}

async fn get_settings(
    crate::auth::Auth(_): crate::auth::Auth,
    State(app): State<AppState>,
) -> ApiResult<Json<ServerSettingsResp>> {
    let s = arx_db::queries::settings::get(&app.db).await?;
    Ok(Json(ServerSettingsResp {
        admin_domain: s.admin_domain,
        acme_email: s.acme_email,
        public_ip: s.public_ip,
    }))
}

#[derive(Deserialize)]
struct PatchSettingsReq {
    #[serde(default)]
    admin_domain: Option<String>,
    #[serde(default)]
    acme_email: Option<String>,
    #[serde(default)]
    public_ip: Option<String>,
}

async fn patch_settings(
    crate::auth::Auth(_): crate::auth::Auth,
    State(app): State<AppState>,
    Json(req): Json<PatchSettingsReq>,
) -> ApiResult<Json<ServerSettingsResp>> {
    if let Some(d) = req.admin_domain {
        if !d.is_empty() {
            arx_build::validate::validate_hostname(&d)
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
        }
        arx_db::queries::settings::set_admin_domain(
            &app.db,
            if d.is_empty() { None } else { Some(&d) },
        )
        .await?;
    }
    if let Some(e) = req.acme_email {
        arx_db::queries::settings::set_acme_email(
            &app.db,
            if e.is_empty() { None } else { Some(&e) },
        )
        .await?;
    }
    if let Some(ip) = req.public_ip {
        arx_db::queries::settings::set_public_ip(
            &app.db,
            if ip.is_empty() { None } else { Some(&ip) },
        )
        .await?;
    }

    crate::deploy::rewrite_traefik(&app).await?;

    let s = arx_db::queries::settings::get(&app.db).await?;
    Ok(Json(ServerSettingsResp {
        admin_domain: s.admin_domain,
        acme_email: s.acme_email,
        public_ip: s.public_ip,
    }))
}

#[derive(Serialize)]
struct SetupStatus {
    eligible: bool,
}

async fn setup_status(State(app): State<AppState>) -> ApiResult<Json<SetupStatus>> {
    let n = auth_q::count_users(&app.db).await?;
    Ok(Json(SetupStatus { eligible: n == 0 }))
}

#[derive(Deserialize)]
struct InstallReq {
    app: GitHubAppFields,

    owner: GitHubOwner,

    #[serde(default = "default_workspace_slug")]
    workspace_slug: String,
    #[serde(default = "default_workspace_name")]
    workspace_name: String,
}

fn default_workspace_slug() -> String {
    "default".into()
}
fn default_workspace_name() -> String {
    "Default".into()
}

#[derive(Deserialize)]
struct GitHubAppFields {
    id: i64,
    slug: String,
    name: String,
    client_id: String,
    client_secret: String,
    webhook_secret: String,
    pem: String,
    html_url: String,
}

#[derive(Deserialize)]
struct GitHubOwner {
    github_user_id: i64,
    github_login: String,
    display_name: String,
    avatar_url: Option<String>,
}

#[derive(Serialize)]
struct InstallResp {
    user: UserResp,
    workspace: WorkspaceResp,
    session_token: String,
}

async fn install_github_app(
    State(app): State<AppState>,
    Json(req): Json<InstallReq>,
) -> ApiResult<Json<InstallResp>> {
    let n = auth_q::count_users(&app.db).await?;
    if n > 0 {
        return Err(ApiError::forbidden());
    }

    gh_q::put_app(
        &app.db,
        &app.master_key,
        &gh_q::AppCreds {
            app_id: req.app.id,
            slug: req.app.slug.clone(),
            name: req.app.name.clone(),
            client_id: req.app.client_id,
            client_secret: req.app.client_secret,
            webhook_secret: req.app.webhook_secret,
            private_key_pem: req.app.pem,
            html_url: req.app.html_url,
        },
    )
    .await?;

    let (user_id, _created) = auth_q::upsert_github_user(
        &app.db,
        req.owner.github_user_id,
        &req.owner.github_login,
        &req.owner.display_name,
        req.owner.avatar_url.as_deref(),
    )
    .await?;

    let ws = workspaces::create(&app.db, &req.workspace_slug, &req.workspace_name, user_id).await?;

    let session = auth_q::issue_session(&app.db, user_id, Some("setup")).await?;

    let _ = arx_db::queries::audit::write(
        &app.db,
        Some(user_id),
        "setup.complete",
        &format!("workspace:{}", ws.slug),
        serde_json::json!({
            "github_app_id": req.app.id,
            "github_app_slug": req.app.slug,
            "github_login": req.owner.github_login,
        }),
    )
    .await;

    Ok(Json(InstallResp {
        user: UserResp {
            id: user_id.as_uuid().to_string(),
            display_name: req.owner.display_name,
            github_login: Some(req.owner.github_login),
        },
        workspace: WorkspaceResp {
            id: ws.id.as_uuid().to_string(),
            slug: ws.slug,
            name: ws.name,
        },
        session_token: session.token_plaintext,
    }))
}
