use crate::auth::Auth;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use arx_core::ids::{EnvironmentId, ProjectId, ServiceId, WorkspaceId};
use arx_core::model::{Project, Role, Service, ServiceSource, Workspace};
use arx_db::queries::{
    auth as auth_q, deployments, domains, environments, members, projects, service_env, services,
    variables, workspaces,
};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/auth/me", get(get_me))
        .route("/v1/auth/logout", post(post_logout))
        .route(
            "/v1/workspaces",
            get(list_workspaces).post(create_workspace),
        )
        .route(
            "/v1/workspaces/:ws",
            get(get_workspace).delete(delete_workspace),
        )
        .route(
            "/v1/workspaces/:ws/members",
            get(list_members).post(invite_member),
        )
        .route("/v1/workspaces/:ws/members/:uid", delete(remove_member))
        .route(
            "/v1/workspaces/:ws/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj",
            get(get_project).delete(delete_project),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/environments",
            get(list_environments).post(create_environment),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services",
            get(list_services).post(create_service),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc",
            get(get_service)
                .patch(rename_service)
                .delete(delete_service_handler),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc/variables",
            get(list_variables).post(set_variable),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc/variables/:key",
            delete(unset_variable),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc/domains",
            get(list_domains).post(add_domain),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc/domains/:dom",
            delete(remove_domain),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc/config",
            get(get_env_config).patch(patch_env_config),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc/deploy",
            post(deploy_service),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc/rollback",
            post(rollback_service),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc/deployments",
            get(list_deployments),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc/logs",
            get(stream_logs),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc/backups",
            get(list_backups).post(backup_now),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc/backups/restore",
            post(restore_backup),
        )
        .route(
            "/v1/workspaces/:ws/projects/:proj/services/:svc/backup-schedule",
            get(get_backup_schedule).put(put_backup_schedule),
        )
        .route(
            "/v1/workspaces/:ws/admin/volumes",
            get(list_volumes_handler),
        )
        .route(
            "/v1/workspaces/:ws/admin/volumes/prune",
            post(prune_volumes_handler),
        )
        .merge(crate::github_routes::routes())
        .merge(crate::setup::routes())
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Serialize)]
pub(crate) struct UserResp {
    pub id: String,
    pub display_name: String,
    pub github_login: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct WorkspaceResp {
    pub id: String,
    pub slug: String,
    pub name: String,
}

async fn get_me(Auth(user): Auth) -> Json<UserResp> {
    Json(UserResp {
        id: user.user_id.as_uuid().to_string(),
        display_name: user.display_name,
        github_login: user.github_login,
    })
}

async fn post_logout(Auth(user): Auth, State(app): State<AppState>) -> ApiResult<()> {
    auth_q::revoke_session(&app.db, user.session_id).await?;
    Ok(())
}

#[derive(Deserialize)]
struct CreateWorkspaceReq {
    slug: String,
    name: String,
}

async fn list_workspaces(
    Auth(user): Auth,
    State(app): State<AppState>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    let list = workspaces::list_for_user(&app.db, user.user_id).await?;
    let out = list
        .into_iter()
        .map(|(w, role)| {
            serde_json::json!({
                "id": w.id.as_uuid().to_string(),
                "slug": w.slug,
                "name": w.name,
                "role": role.as_str(),
            })
        })
        .collect();
    Ok(Json(out))
}

async fn create_workspace(
    Auth(user): Auth,
    State(app): State<AppState>,
    Json(req): Json<CreateWorkspaceReq>,
) -> ApiResult<Json<WorkspaceResp>> {
    let w = workspaces::create(&app.db, &req.slug, &req.name, user.user_id).await?;
    Ok(Json(WorkspaceResp {
        id: w.id.as_uuid().to_string(),
        slug: w.slug,
        name: w.name,
    }))
}

async fn require_workspace_role(
    app: &AppState,
    user_id: arx_core::ids::UserId,
    ws_slug: &str,
) -> Result<(WorkspaceId, Role), ApiError> {
    let w = workspaces::get_by_slug(&app.db, ws_slug).await?;
    let role = auth_q::workspace_role(&app.db, w.id, user_id).await?;
    Ok((w.id, role))
}

async fn require_admin(
    app: &AppState,
    user_id: arx_core::ids::UserId,
    ws_slug: &str,
) -> Result<WorkspaceId, ApiError> {
    let (ws_id, role) = require_workspace_role(app, user_id, ws_slug).await?;
    if role != Role::Admin {
        return Err(ApiError::forbidden());
    }
    Ok(ws_id)
}

#[derive(Deserialize, Default)]
struct DeleteQuery {
    #[serde(default)]
    force: bool,
    #[serde(default)]
    with_data: bool,
}

async fn delete_workspace(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path(ws): Path<String>,
    axum::extract::Query(q): axum::extract::Query<DeleteQuery>,
) -> ApiResult<()> {
    let ws_id = require_admin(&app, user.user_id, &ws).await?;
    crate::cascade::delete_workspace(
        &app,
        user.user_id,
        ws_id,
        &ws,
        crate::cascade::DeleteOpts {
            force: q.force,
            with_data: q.with_data,
        },
    )
    .await?;
    if q.force {
        let _ = arx_db::queries::audit::write(
            &app.db,
            Some(user.user_id),
            "workspace.delete_force",
            &format!("workspace:{ws}"),
            serde_json::json!({"with_data": q.with_data}),
        )
        .await;
    }
    Ok(())
}

async fn delete_project(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj)): Path<(String, String)>,
    axum::extract::Query(q): axum::extract::Query<DeleteQuery>,
) -> ApiResult<()> {
    let (ws_id, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let p = projects::get_by_slug(&app.db, ws_id, &proj).await?;
    crate::cascade::delete_project(
        &app,
        user.user_id,
        p.id,
        &proj,
        crate::cascade::DeleteOpts {
            force: q.force,
            with_data: q.with_data,
        },
    )
    .await?;
    if q.force {
        let _ = arx_db::queries::audit::write(
            &app.db,
            Some(user.user_id),
            "project.delete_force",
            &format!("workspace:{ws}/project:{proj}"),
            serde_json::json!({"with_data": q.with_data}),
        )
        .await;
    }
    Ok(())
}

async fn delete_service_handler(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    axum::extract::Query(q): axum::extract::Query<DeleteQuery>,
) -> ApiResult<()> {
    let (ws_id, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let p = projects::get_by_slug(&app.db, ws_id, &proj).await?;
    crate::cascade::delete_service_by_slug(
        &app,
        user.user_id,
        p.id,
        &svc,
        crate::cascade::DeleteOpts {
            force: q.force,
            with_data: q.with_data,
        },
    )
    .await?;
    if q.force {
        let _ = arx_db::queries::audit::write(
            &app.db,
            Some(user.user_id),
            "service.delete_force",
            &format!("workspace:{ws}/project:{proj}/service:{svc}"),
            serde_json::json!({"with_data": q.with_data}),
        )
        .await;
    }
    Ok(())
}

async fn get_workspace(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path(ws): Path<String>,
) -> ApiResult<Json<WorkspaceResp>> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let w = workspaces::get_by_slug(&app.db, &ws).await?;
    Ok(Json(WorkspaceResp {
        id: w.id.as_uuid().to_string(),
        slug: w.slug,
        name: w.name,
    }))
}

#[derive(Deserialize)]
struct InviteMemberReq {
    github_login: String,
    role: String,
}

async fn list_members(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path(ws): Path<String>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    let (ws_id, _role) = require_workspace_role(&app, user.user_id, &ws).await?;
    let list = members::list(&app.db, ws_id).await?;
    let out = list
        .into_iter()
        .map(|m| {
            serde_json::json!({
                "user_id": m.user_id.as_uuid().to_string(),
                "display_name": m.display_name,
                "github_login": m.github_login,
                "role": m.role.as_str(),
            })
        })
        .collect();
    Ok(Json(out))
}

async fn invite_member(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path(ws): Path<String>,
    Json(req): Json<InviteMemberReq>,
) -> ApiResult<()> {
    let ws_id = require_admin(&app, user.user_id, &ws).await?;
    let role = match req.role.as_str() {
        "admin" => Role::Admin,
        "member" => Role::Member,
        other => return Err(ApiError::bad_request(format!("unknown role: {other}"))),
    };
    members::invite_or_add(&app.db, ws_id, user.user_id, &req.github_login, role).await?;
    Ok(())
}

async fn remove_member(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, uid)): Path<(String, String)>,
) -> ApiResult<()> {
    let ws_id = require_admin(&app, user.user_id, &ws).await?;
    let uid = uuid::Uuid::parse_str(&uid).map_err(|_| ApiError::bad_request("bad uuid"))?;
    members::remove(&app.db, ws_id, arx_core::ids::UserId::from_uuid(uid)).await?;
    Ok(())
}

#[derive(Deserialize)]
struct CreateProjectReq {
    slug: String,
    name: String,
}

#[derive(Serialize)]
struct ProjectResp {
    id: String,
    slug: String,
    name: String,
}

async fn list_projects(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path(ws): Path<String>,
) -> ApiResult<Json<Vec<ProjectResp>>> {
    let (ws_id, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let list = projects::list_in_workspace(&app.db, ws_id).await?;
    Ok(Json(
        list.into_iter()
            .map(|p| ProjectResp {
                id: p.id.as_uuid().to_string(),
                slug: p.slug,
                name: p.name,
            })
            .collect(),
    ))
}

async fn create_project(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path(ws): Path<String>,
    Json(req): Json<CreateProjectReq>,
) -> ApiResult<Json<ProjectResp>> {
    let (ws_id, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let p = projects::create(&app.db, ws_id, &req.slug, &req.name).await?;
    Ok(Json(ProjectResp {
        id: p.id.as_uuid().to_string(),
        slug: p.slug,
        name: p.name,
    }))
}

async fn get_project(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj)): Path<(String, String)>,
) -> ApiResult<Json<ProjectResp>> {
    let (ws_id, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let p = projects::get_by_slug(&app.db, ws_id, &proj).await?;
    Ok(Json(ProjectResp {
        id: p.id.as_uuid().to_string(),
        slug: p.slug,
        name: p.name,
    }))
}

#[derive(Deserialize)]
struct CreateEnvReq {
    slug: String,
    name: String,
}

#[derive(Serialize)]
struct EnvResp {
    id: String,
    slug: String,
    name: String,
    is_default: bool,
}

async fn list_environments(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj)): Path<(String, String)>,
) -> ApiResult<Json<Vec<EnvResp>>> {
    let (ws_id, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let p = projects::get_by_slug(&app.db, ws_id, &proj).await?;
    let list = environments::list_in_project(&app.db, p.id).await?;
    Ok(Json(
        list.into_iter()
            .map(|e| EnvResp {
                id: e.id.as_uuid().to_string(),
                slug: e.slug,
                name: e.name,
                is_default: e.is_default,
            })
            .collect(),
    ))
}

async fn create_environment(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj)): Path<(String, String)>,
    Json(req): Json<CreateEnvReq>,
) -> ApiResult<Json<EnvResp>> {
    let (ws_id, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let p = projects::get_by_slug(&app.db, ws_id, &proj).await?;
    let e = environments::create(&app.db, p.id, &req.slug, &req.name).await?;
    Ok(Json(EnvResp {
        id: e.id.as_uuid().to_string(),
        slug: e.slug,
        name: e.name,
        is_default: e.is_default,
    }))
}

#[derive(Deserialize)]
struct CreateServiceReq {
    slug: String,
    name: String,
    source: ServiceSource,
    #[serde(default)]
    build_command: Option<String>,
    #[serde(default)]
    start_command: Option<String>,
}

#[derive(Serialize)]
struct ServiceResp {
    id: String,
    slug: String,
    name: String,
    kind: &'static str,
    source: ServiceSource,
    build_command: Option<String>,
    start_command: Option<String>,
}

impl From<arx_core::model::Service> for ServiceResp {
    fn from(s: arx_core::model::Service) -> Self {
        ServiceResp {
            id: s.id.as_uuid().to_string(),
            slug: s.slug,
            name: s.name,
            kind: s.kind.as_str(),
            source: s.source,
            build_command: s.build_command,
            start_command: s.start_command,
        }
    }
}

async fn list_services(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj)): Path<(String, String)>,
) -> ApiResult<Json<Vec<ServiceResp>>> {
    let (ws_id, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let p = projects::get_by_slug(&app.db, ws_id, &proj).await?;
    let list = services::list_in_project(&app.db, p.id).await?;
    Ok(Json(list.into_iter().map(ServiceResp::from).collect()))
}

async fn create_service(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj)): Path<(String, String)>,
    Json(req): Json<CreateServiceReq>,
) -> ApiResult<Json<ServiceResp>> {
    let (ws_id, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let p = projects::get_by_slug(&app.db, ws_id, &proj).await?;
    let s = services::create(
        &app.db,
        p.id,
        &req.slug,
        &req.name,
        &req.source,
        req.build_command.as_deref(),
        req.start_command.as_deref(),
    )
    .await?;
    Ok(Json(s.into()))
}

/// Tri-state PATCH: absent = leave alone, `null` = clear, value = set.
#[derive(Deserialize, Default)]
struct PatchServiceReq {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_some")]
    build_command: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    start_command: Option<Option<String>>,
}

fn deserialize_some<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

async fn rename_service(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    Json(req): Json<PatchServiceReq>,
) -> ApiResult<Json<ServiceResp>> {
    let (ws_id, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let p = projects::get_by_slug(&app.db, ws_id, &proj).await?;
    let s = services::get_by_slug(&app.db, p.id, &svc).await?;
    let patch = arx_db::queries::services::ServicePatch {
        name: req.name,
        build_command: req.build_command,
        start_command: req.start_command,
    };
    services::update(&app.db, s.id, &patch).await?;
    let updated = services::get_by_id(&app.db, s.id).await?;
    Ok(Json(updated.into()))
}

async fn get_service(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
) -> ApiResult<Json<ServiceResp>> {
    let (ws_id, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let p = projects::get_by_slug(&app.db, ws_id, &proj).await?;
    let s = services::get_by_slug(&app.db, p.id, &svc).await?;
    Ok(Json(s.into()))
}

#[derive(Deserialize)]
struct SetVarReq {
    key: String,
    value: String,
    #[serde(default)]
    sealed: bool,

    #[serde(default)]
    env: Option<String>,
}

#[derive(Serialize)]
struct VarResp {
    key: String,
    value: Option<String>,
    sealed: bool,
}

async fn resolve_se(
    app: &AppState,
    ws_slug: &str,
    proj_slug: &str,
    svc_slug: &str,
    env_slug: Option<&str>,
) -> Result<(ServiceId, EnvironmentId, ProjectId), ApiError> {
    let w = workspaces::get_by_slug(&app.db, ws_slug).await?;
    let p = projects::get_by_slug(&app.db, w.id, proj_slug).await?;
    let s = services::get_by_slug(&app.db, p.id, svc_slug).await?;
    let env_slug = env_slug.unwrap_or("production");
    let e = environments::get_by_slug(&app.db, p.id, env_slug).await?;
    Ok((s.id, e.id, p.id))
}

async fn resolve_wps(
    app: &AppState,
    ws_slug: &str,
    proj_slug: &str,
    svc_slug: &str,
) -> Result<(Workspace, Project, Service), ApiError> {
    let w = workspaces::get_by_slug(&app.db, ws_slug).await?;
    let p = projects::get_by_slug(&app.db, w.id, proj_slug).await?;
    let s = services::get_by_slug(&app.db, p.id, svc_slug).await?;
    Ok((w, p, s))
}

#[derive(Deserialize)]
struct EnvQuery {
    #[serde(default)]
    env: Option<String>,
}

async fn list_variables(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    axum::extract::Query(q): axum::extract::Query<EnvQuery>,
) -> ApiResult<Json<Vec<VarResp>>> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (sid, eid, _) = resolve_se(&app, &ws, &proj, &svc, q.env.as_deref()).await?;
    let list = variables::list(&app.db, &app.master_key, sid, eid).await?;
    Ok(Json(
        list.into_iter()
            .map(|v| VarResp {
                key: v.key,
                value: v.plaintext,
                sealed: v.sealed,
            })
            .collect(),
    ))
}

async fn set_variable(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    Json(req): Json<SetVarReq>,
) -> ApiResult<()> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (sid, eid, _) = resolve_se(&app, &ws, &proj, &svc, req.env.as_deref()).await?;
    let outcome = variables::set(
        &app.db,
        &app.master_key,
        sid,
        eid,
        &req.key,
        &req.value,
        req.sealed,
    )
    .await?;
    let env_label = req.env.clone().unwrap_or_else(|| "production".into());
    if outcome.replaced_sealed {
        let _ = arx_db::queries::audit::write(
            &app.db,
            Some(user.user_id),
            "variable.replace_sealed",
            &format!("service:{}/var:{}", svc, req.key),
            serde_json::json!({"env": env_label}),
        )
        .await;
    } else if req.sealed {
        let _ = arx_db::queries::audit::write(
            &app.db,
            Some(user.user_id),
            "variable.seal",
            &format!("service:{}/var:{}", svc, req.key),
            serde_json::json!({"env": env_label}),
        )
        .await;
    }
    Ok(())
}

async fn unset_variable(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc, key)): Path<(String, String, String, String)>,
    axum::extract::Query(q): axum::extract::Query<EnvQuery>,
) -> ApiResult<()> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (sid, eid, _) = resolve_se(&app, &ws, &proj, &svc, q.env.as_deref()).await?;
    variables::unset(&app.db, sid, eid, &key).await?;
    Ok(())
}

#[derive(Deserialize)]
struct AddDomainReq {
    hostname: String,
    #[serde(default)]
    env: Option<String>,
}

#[derive(Serialize)]
struct DomainResp {
    id: String,
    hostname: String,
    verified: bool,
    cert_status: &'static str,
}

async fn list_domains(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    axum::extract::Query(q): axum::extract::Query<EnvQuery>,
) -> ApiResult<Json<Vec<DomainResp>>> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (sid, eid, _) = resolve_se(&app, &ws, &proj, &svc, q.env.as_deref()).await?;
    let list = domains::list_for_service_env(&app.db, sid, eid).await?;
    Ok(Json(
        list.into_iter()
            .map(|d| DomainResp {
                id: d.id.as_uuid().to_string(),
                hostname: d.hostname,
                verified: d.verified,
                cert_status: d.cert_status.as_str(),
            })
            .collect(),
    ))
}

async fn add_domain(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    Json(req): Json<AddDomainReq>,
) -> ApiResult<Json<DomainResp>> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (sid, eid, _) = resolve_se(&app, &ws, &proj, &svc, req.env.as_deref()).await?;

    arx_build::validate::validate_hostname(&req.hostname)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    if let Some(expected_ip) = app.config.server.public_ip {
        if let Err(e) = crate::dns_verify::verify_a_record(&req.hostname, expected_ip).await {
            return Err(ApiError::bad_request(format!(
                "DNS verification failed for {}: {} \
                 — set an A record for `{}` pointing to {}",
                req.hostname, e, req.hostname, expected_ip
            )));
        }
    }

    let d = domains::add(&app.db, sid, eid, &req.hostname).await?;
    crate::deploy::rewrite_traefik(&app).await?;

    Ok(Json(DomainResp {
        id: d.id.as_uuid().to_string(),
        hostname: d.hostname,
        verified: d.verified,
        cert_status: d.cert_status.as_str(),
    }))
}

async fn remove_domain(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc, dom)): Path<(String, String, String, String)>,
) -> ApiResult<()> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (_, _, s) = resolve_wps(&app, &ws, &proj, &svc).await?;
    let id = uuid::Uuid::parse_str(&dom).map_err(|_| ApiError::bad_request("bad uuid"))?;
    domains::remove_scoped(&app.db, s.id, arx_core::ids::DomainId::from_uuid(id)).await?;
    crate::deploy::rewrite_traefik(&app).await?;
    Ok(())
}

#[derive(Deserialize)]
struct DeployReq {
    #[serde(default)]
    env: Option<String>,
}

#[derive(Serialize)]
struct DeploymentResp {
    id: String,
    status: &'static str,
    image_ref: Option<String>,
    commit_sha: Option<String>,
    container_id: Option<String>,
    error: Option<String>,
    created_at: String,
}

async fn deploy_service(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    Json(req): Json<DeployReq>,
) -> ApiResult<Json<DeploymentResp>> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (w, p, s) = resolve_wps(&app, &ws, &proj, &svc).await?;
    let env_slug = req.env.as_deref().unwrap_or("production");
    let e = environments::get_by_slug(&app.db, p.id, env_slug).await?;

    let dep_id = deployments::create_pending(
        &app.db,
        s.id,
        e.id,
        None,
        None,
        &serde_json::Value::Object(Default::default()),
    )
    .await?;

    let app_bg = app.clone();
    tokio::spawn(async move {
        match crate::deploy::deploy_with_existing(&app_bg, &w, &p, &s, &e, dep_id).await {
            Ok(d) => tracing::info!(
                deployment_id = %d.id.as_uuid(),
                status = ?d.status,
                "background deploy finished"
            ),
            Err(e) => {
                tracing::error!(error = ?e, "background deploy failed");
                let _ = arx_db::queries::deployments::update_status(
                    &app_bg.db,
                    dep_id,
                    arx_core::model::DeploymentStatus::Failed,
                    None,
                    Some(&format!("{:?}", e.2)),
                    true,
                )
                .await;
            }
        }
    });

    let d = deployments::get(&app.db, dep_id).await?;
    Ok(Json(DeploymentResp {
        id: d.id.as_uuid().to_string(),
        status: d.status.as_str(),
        image_ref: d.image_ref,
        commit_sha: d.commit_sha,
        container_id: d.container_id,
        error: d.error,
        created_at: d.created_at.to_rfc3339(),
    }))
}

#[derive(Serialize, Deserialize)]
struct EnvConfigResp {
    cpu_limit: Option<f64>,
    memory_limit_mb: Option<i64>,
    healthcheck_path: Option<String>,
    healthcheck_timeout_seconds: i32,
}

async fn get_env_config(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    axum::extract::Query(q): axum::extract::Query<EnvQuery>,
) -> ApiResult<Json<EnvConfigResp>> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (sid, eid, _) = resolve_se(&app, &ws, &proj, &svc, q.env.as_deref()).await?;
    let c = service_env::get(&app.db, sid, eid).await?;
    Ok(Json(EnvConfigResp {
        cpu_limit: c.cpu_limit,
        memory_limit_mb: c.memory_limit_mb,
        healthcheck_path: c.healthcheck_path,
        healthcheck_timeout_seconds: c.healthcheck_timeout_seconds,
    }))
}

#[derive(Deserialize)]
struct PatchEnvConfigReq {
    #[serde(default)]
    env: Option<String>,
    #[serde(default)]
    cpu_limit: Option<f64>,
    #[serde(default)]
    memory_limit_mb: Option<i64>,
    #[serde(default)]
    healthcheck_path: Option<String>,
    #[serde(default)]
    healthcheck_timeout_seconds: Option<i32>,
}

async fn patch_env_config(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    Json(req): Json<PatchEnvConfigReq>,
) -> ApiResult<Json<EnvConfigResp>> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (sid, eid, _) = resolve_se(&app, &ws, &proj, &svc, req.env.as_deref()).await?;
    service_env::update(
        &app.db,
        sid,
        eid,
        service_env::EnvConfigPatch {
            cpu_limit: req.cpu_limit,
            memory_limit_mb: req.memory_limit_mb,
            healthcheck_path: req.healthcheck_path,
            healthcheck_timeout_seconds: req.healthcheck_timeout_seconds,
        },
    )
    .await?;
    let c = service_env::get(&app.db, sid, eid).await?;
    Ok(Json(EnvConfigResp {
        cpu_limit: c.cpu_limit,
        memory_limit_mb: c.memory_limit_mb,
        healthcheck_path: c.healthcheck_path,
        healthcheck_timeout_seconds: c.healthcheck_timeout_seconds,
    }))
}

#[derive(Serialize)]
struct BackupResp {
    id: String,
    size_bytes: i64,
    storage_uri: String,
    created_at: String,
}

async fn list_backups(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
) -> ApiResult<Json<Vec<BackupResp>>> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (_, _, s) = resolve_wps(&app, &ws, &proj, &svc).await?;
    let list = crate::backups::list_records(&app, s.id).await?;
    Ok(Json(
        list.into_iter()
            .map(|b| BackupResp {
                id: b.id.to_string(),
                size_bytes: b.size_bytes,
                storage_uri: b.storage_uri,
                created_at: b.created_at.to_rfc3339(),
            })
            .collect(),
    ))
}

async fn backup_now(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
) -> ApiResult<Json<BackupResp>> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (_, _, s) = resolve_wps(&app, &ws, &proj, &svc).await?;
    let r = crate::backups::backup_now(&app, &s).await?;
    Ok(Json(BackupResp {
        id: r.id.to_string(),
        size_bytes: r.size_bytes,
        storage_uri: r.storage_uri,
        created_at: chrono::Utc::now().to_rfc3339(),
    }))
}

#[derive(Deserialize)]
struct RestoreReq {
    storage_uri: String,
}

async fn restore_backup(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    Json(req): Json<RestoreReq>,
) -> ApiResult<()> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (_, _, s) = resolve_wps(&app, &ws, &proj, &svc).await?;
    crate::backups::restore(&app, &s, &req.storage_uri).await?;
    let _ = arx_db::queries::audit::write(
        &app.db,
        Some(user.user_id),
        "backup.restore",
        &format!("service:{}", s.slug),
        serde_json::json!({"storage_uri": req.storage_uri}),
    )
    .await;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct ScheduleResp {
    cron_expression: String,
    retention_count: i32,
    storage: String,
    enabled: bool,
}

async fn get_backup_schedule(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
) -> ApiResult<Json<Option<ScheduleResp>>> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (_, _, s) = resolve_wps(&app, &ws, &proj, &svc).await?;
    let sch = arx_db::queries::backups::get_schedule(&app.db, s.id).await?;
    Ok(Json(sch.map(|s| ScheduleResp {
        cron_expression: s.cron_expression,
        retention_count: s.retention_count,
        storage: s.storage,
        enabled: s.enabled,
    })))
}

async fn put_backup_schedule(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    Json(req): Json<ScheduleResp>,
) -> ApiResult<()> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (_, _, s) = resolve_wps(&app, &ws, &proj, &svc).await?;
    arx_db::queries::backups::upsert_schedule(
        &app.db,
        s.id,
        &req.cron_expression,
        req.retention_count,
        &req.storage,
        req.enabled,
    )
    .await?;
    Ok(())
}

#[derive(Deserialize)]
struct RollbackReq {
    deployment_id: String,
    #[serde(default)]
    env: Option<String>,
}

async fn rollback_service(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    Json(req): Json<RollbackReq>,
) -> ApiResult<Json<DeploymentResp>> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (w, p, s) = resolve_wps(&app, &ws, &proj, &svc).await?;
    let env_slug = req.env.as_deref().unwrap_or("production");
    let e = environments::get_by_slug(&app.db, p.id, env_slug).await?;

    let dep_uuid = uuid::Uuid::parse_str(&req.deployment_id)
        .map_err(|_| ApiError::bad_request("bad deployment_id"))?;
    let target =
        deployments::get(&app.db, arx_core::ids::DeploymentId::from_uuid(dep_uuid)).await?;
    if target.service_id != s.id || target.environment_id != e.id {
        return Err(ApiError::bad_request(
            "deployment belongs to a different service/environment",
        ));
    }

    let image = target
        .image_ref
        .ok_or_else(|| ApiError::bad_request("target deployment has no image to roll back to"))?;

    let d = match &s.source {
        arx_core::model::ServiceSource::DockerImage { .. }
        | arx_core::model::ServiceSource::GitSource { .. } => {
            crate::deploy::deploy_docker_image(
                &app,
                crate::deploy::DeployContext {
                    workspace: &w,
                    project: &p,
                    service: &s,
                    environment: &e,
                    existing_dep_id: None,
                    image,
                    extra_env: vec![],
                    extra_mounts: vec![],
                },
            )
            .await?
        }
        arx_core::model::ServiceSource::DbTemplate { .. } => {
            return Err(ApiError::bad_request(
                "rollback on DB template not supported",
            ));
        }
    };

    let _ = arx_db::queries::audit::write(
        &app.db,
        Some(user.user_id),
        "deployment.rollback",
        &format!("service:{}", s.slug),
        serde_json::json!({
            "from_deployment": req.deployment_id,
            "new_deployment": d.id.as_uuid().to_string(),
            "env": e.slug,
        }),
    )
    .await;

    Ok(Json(DeploymentResp {
        id: d.id.as_uuid().to_string(),
        status: d.status.as_str(),
        image_ref: d.image_ref,
        commit_sha: d.commit_sha,
        container_id: d.container_id,
        error: d.error,
        created_at: d.created_at.to_rfc3339(),
    }))
}

#[derive(Deserialize)]
struct LogQuery {
    #[serde(default)]
    env: Option<String>,
    #[serde(default)]
    follow: bool,
}

async fn stream_logs(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    axum::extract::Query(q): axum::extract::Query<LogQuery>,
) -> ApiResult<axum::response::Response> {
    use arx_docker::ContainerEngine;
    use axum::response::sse::{Event, Sse};
    use futures::StreamExt;

    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (sid, eid, _) = resolve_se(&app, &ws, &proj, &svc, q.env.as_deref()).await?;

    use sqlx::Row;
    let row = sqlx::query(
        "SELECT container_id FROM deployments
         WHERE service_id = ? AND environment_id = ? AND status = 'live'
            AND container_id IS NOT NULL
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(sid.as_uuid().to_string())
    .bind(eid.as_uuid().to_string())
    .fetch_optional(&app.db)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let row = row.ok_or_else(ApiError::not_found)?;
    let container_id: String = row
        .try_get("container_id")
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let handle = arx_docker::ContainerHandle(container_id);
    let stream = app
        .docker
        .logs(&handle, q.follow)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let sse_stream = stream.map(|line| {
        let body = match line {
            Ok(l) => serde_json::json!({
                "stream": format!("{:?}", l.stream),
                "line": l.line.trim_end(),
                "ts": l.timestamp.to_rfc3339(),
            })
            .to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        };
        Ok::<_, std::convert::Infallible>(Event::default().data(body))
    });

    Ok(Sse::new(sse_stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response())
}

async fn list_deployments(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path((ws, proj, svc)): Path<(String, String, String)>,
    axum::extract::Query(q): axum::extract::Query<EnvQuery>,
) -> ApiResult<Json<Vec<DeploymentResp>>> {
    let (_, _) = require_workspace_role(&app, user.user_id, &ws).await?;
    let (sid, eid, _) = resolve_se(&app, &ws, &proj, &svc, q.env.as_deref()).await?;
    let list = deployments::list_for_service_env(&app.db, sid, eid, 50).await?;
    Ok(Json(
        list.into_iter()
            .map(|d| DeploymentResp {
                id: d.id.as_uuid().to_string(),
                status: d.status.as_str(),
                image_ref: d.image_ref,
                commit_sha: d.commit_sha,
                container_id: d.container_id,
                error: d.error,
                created_at: d.created_at.to_rfc3339(),
            })
            .collect(),
    ))
}

#[derive(Deserialize, Default)]
struct PruneQuery {
    #[serde(default)]
    execute: bool,
}

async fn list_volumes_handler(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path(ws): Path<String>,
) -> ApiResult<Json<Vec<crate::volumes::VolumeReport>>> {
    let _ = require_admin(&app, user.user_id, &ws).await?;
    let reports = crate::volumes::list(&app).await?;
    Ok(Json(reports))
}

async fn prune_volumes_handler(
    Auth(user): Auth,
    State(app): State<AppState>,
    Path(ws): Path<String>,
    axum::extract::Query(q): axum::extract::Query<PruneQuery>,
) -> ApiResult<Json<crate::volumes::PruneResult>> {
    let _ = require_admin(&app, user.user_id, &ws).await?;
    let result = crate::volumes::prune(&app, !q.execute).await?;
    let _ = arx_db::queries::audit::write(
        &app.db,
        Some(user.user_id),
        "volumes.prune",
        &format!("workspace:{ws}"),
        serde_json::json!({
            "execute": q.execute,
            "removed": result.removed.len(),
            "skipped": result.skipped.len(),
        }),
    )
    .await;
    Ok(Json(result))
}
