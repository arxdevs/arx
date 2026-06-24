use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use arx_build::validate;
use arx_core::ids::{EnvironmentId, ProjectId, ServiceId};
use arx_core::model::{
    Deployment, DeploymentStatus, Environment, Project, Service, ServiceSource, Workspace,
};
use arx_db::queries::{deployments, domains, service_env, services as svc_q};
use arx_docker::{
    ContainerEngine, ContainerSpec, Mount, PortBinding, Protocol, ResourceLimits, RestartPolicy,
};
use arx_traefik::{BackendTarget, Route};
use futures::FutureExt;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{error, info, warn};

pub(crate) struct DeployContext<'a> {
    pub workspace: &'a Workspace,
    pub project: &'a Project,
    pub service: &'a Service,
    pub environment: &'a Environment,
    pub existing_dep_id: Option<arx_core::ids::DeploymentId>,
    pub image: String,
    pub extra_env: Vec<(String, String)>,
    pub extra_mounts: Vec<Mount>,
}

pub async fn deploy(
    app: &AppState,
    workspace: &Workspace,
    project: &Project,
    service: &Service,
    environment: &Environment,
) -> ApiResult<Deployment> {
    deploy_with_optional_id(app, workspace, project, service, environment, None).await
}

pub async fn deploy_with_existing(
    app: &AppState,
    workspace: &Workspace,
    project: &Project,
    service: &Service,
    environment: &Environment,
    dep_id: arx_core::ids::DeploymentId,
) -> ApiResult<Deployment> {
    deploy_with_optional_id(app, workspace, project, service, environment, Some(dep_id)).await
}

async fn deploy_with_optional_id(
    app: &AppState,
    workspace: &Workspace,
    project: &Project,
    service: &Service,
    environment: &Environment,
    existing_dep_id: Option<arx_core::ids::DeploymentId>,
) -> ApiResult<Deployment> {
    run_with_events(
        app,
        workspace,
        project,
        service,
        environment,
        arx_core::model::DeployTrigger::Deploy,
        deploy_inner(app, workspace, project, service, environment, existing_dep_id),
    )
    .await
}

/// Builds the slug-only event context for outgoing webhook emission.
pub(crate) fn event_ctx(
    workspace: &Workspace,
    project: &Project,
    service: &Service,
    environment: &Environment,
) -> crate::webhooks::DeployEventCtx {
    crate::webhooks::DeployEventCtx {
        workspace_id: workspace.id,
        workspace_slug: workspace.slug.clone(),
        project_id: project.id,
        project_slug: project.slug.clone(),
        service_slug: service.slug.clone(),
        environment_slug: environment.slug.clone(),
    }
}

/// Maps an `ApiError` to a coarse, secret-free failure reason for webhook
/// payloads. Never includes raw error text (which can carry build/git/docker
/// stderr and thus secrets).
pub(crate) fn classify_failure(err: &ApiError) -> &'static str {
    match err.0 {
        axum::http::StatusCode::BAD_REQUEST => "invalid_request",
        _ => "deploy_failed",
    }
}

/// Wraps a deploy future with terminal-or-nothing outgoing-webhook emission:
/// emits `started` before, then exactly one terminal event from the result.
/// Emission is fire-and-forget and never alters the deploy result.
pub(crate) async fn run_with_events(
    app: &AppState,
    workspace: &Workspace,
    project: &Project,
    service: &Service,
    environment: &Environment,
    trigger: arx_core::model::DeployTrigger,
    fut: impl std::future::Future<Output = ApiResult<Deployment>>,
) -> ApiResult<Deployment> {
    let ctx = event_ctx(workspace, project, service, environment);
    crate::webhooks::emit_deploy_started(app, &ctx, trigger).await;
    // Guard against a panic in the deploy future leaving `started` with no
    // terminal event: emit `failed`, then resume the panic so existing
    // behaviour (task abort) is unchanged.
    let result = match std::panic::AssertUnwindSafe(fut)
        .catch_unwind()
        .await
    {
        Ok(r) => r,
        Err(panic) => {
            crate::webhooks::emit_deploy_terminal(
                app,
                &ctx,
                trigger,
                None,
                false,
                Some("panicked"),
            )
            .await;
            std::panic::resume_unwind(panic);
        }
    };
    match &result {
        Ok(d) => {
            crate::webhooks::emit_deploy_terminal(
                app,
                &ctx,
                trigger,
                Some(&d.id.as_uuid().to_string()),
                true,
                None,
            )
            .await;
        }
        Err(e) => {
            crate::webhooks::emit_deploy_terminal(
                app,
                &ctx,
                trigger,
                None,
                false,
                Some(classify_failure(e)),
            )
            .await;
        }
    }
    result
}

async fn deploy_inner(
    app: &AppState,
    workspace: &Workspace,
    project: &Project,
    service: &Service,
    environment: &Environment,
    existing_dep_id: Option<arx_core::ids::DeploymentId>,
) -> ApiResult<Deployment> {
    match &service.source {
        ServiceSource::DockerImage { image, .. } => {
            deploy_docker_image(
                app,
                DeployContext {
                    workspace,
                    project,
                    service,
                    environment,
                    existing_dep_id,
                    image: image.clone(),
                    extra_env: vec![],
                    extra_mounts: vec![],
                },
            )
            .await
        }
        ServiceSource::DbTemplate { template, version } => {
            if let Some(id) = existing_dep_id {
                let _ = deployments::update_status(
                    &app.db,
                    id,
                    arx_core::model::DeploymentStatus::Superseded,
                    None,
                    None,
                    true,
                )
                .await;
            }
            crate::db_template::deploy(
                app,
                workspace,
                project,
                service,
                environment,
                *template,
                version.as_deref(),
            )
            .await
        }
        ServiceSource::GitSource {
            github_repo,
            branch,
            dockerfile,
            root_directory,
            watch_paths: _,
        } => {
            deploy_git_source(
                app,
                workspace,
                project,
                service,
                environment,
                github_repo,
                branch,
                dockerfile.as_deref(),
                root_directory.as_deref(),
                existing_dep_id,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn deploy_git_source(
    app: &AppState,
    workspace: &Workspace,
    project: &Project,
    service: &Service,
    environment: &Environment,
    github_repo: &str,
    branch: &str,
    dockerfile: Option<&str>,
    root_directory: Option<&str>,
    existing_dep_id: Option<arx_core::ids::DeploymentId>,
) -> ApiResult<Deployment> {
    let github_repo = validate::validate_github_repo(github_repo)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let branch =
        validate::validate_git_ref(branch).map_err(|e| ApiError::bad_request(e.to_string()))?;

    let url = format!("https://github.com/{github_repo}.git");
    let cloner = arx_build::Cloner::new(app.config.paths.repos_dir.clone());
    let key = format!("{}-{}", service.id.as_uuid(), sanitize(branch));

    // Authenticate the clone with an installation token when one covers this
    // repo; public repos (no mapped installation) fall back to anonymous.
    let token = crate::github_sync::clone_token_for_repo(app, github_repo).await?;

    let (path, sha) = cloner
        .checkout(
            &key,
            &arx_build::GitOpts {
                url,
                branch: branch.to_string(),
                token,
            },
        )
        .await
        .map_err(|e| ApiError::internal(format!("git checkout: {e}")))?;

    let sha_short = sha
        .get(..sha.len().min(12))
        .ok_or_else(|| ApiError::internal("empty git sha"))?;
    validate::validate_sha_hex(sha_short).map_err(|e| ApiError::internal(e.to_string()))?;

    // Resolve service variables before the build so they are available at build
    // time (Railway parity). The same set is re-resolved for the container at
    // runtime in deploy_docker_image. Sorted for a deterministic config hash.
    let mut build_env =
        crate::var_resolve::resolve_for_injection(app, project.id, service.id, environment.id)
            .await?;
    build_env.sort();

    let config_hash = compute_config_hash(
        service.build_command.as_deref(),
        service.start_command.as_deref(),
        dockerfile,
        root_directory,
        &build_env,
    );
    let image_tag = format!(
        "arx-svc-{}:{}-{}",
        service.id.as_uuid().simple(),
        sha_short,
        &config_hash[..6]
    );

    let out = arx_build::build(&arx_build::BuildInput {
        source_dir: path,
        image_tag: image_tag.clone(),
        dockerfile: dockerfile.map(std::path::PathBuf::from),
        root_directory: root_directory.map(std::path::PathBuf::from),
        build_command: service.build_command.clone(),
        start_command: service.start_command.clone(),
        build_env,
    })
    .await
    .map_err(|e| match e {
        arx_core::Error::InvalidInput(m) => ApiError::bad_request(m),
        other => ApiError::internal(format!("build: {other}")),
    })?;

    tracing::info!(image = %out.image_ref, builder = ?out.used, "git source built");

    deploy_docker_image(
        app,
        DeployContext {
            workspace,
            project,
            service,
            environment,
            existing_dep_id,
            image: out.image_ref,
            extra_env: vec![("ARX_COMMIT_SHA".into(), sha.clone())],
            extra_mounts: vec![],
        },
    )
    .await
}

fn compute_config_hash(
    build_cmd: Option<&str>,
    start_cmd: Option<&str>,
    dockerfile: Option<&str>,
    root_directory: Option<&str>,
    build_env: &[(String, String)],
) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for part in [build_cmd, start_cmd, dockerfile, root_directory] {
        h.update(part.unwrap_or("").as_bytes());
        h.update([0u8]);
    }
    // Build-time env is part of the build identity: a changed variable must
    // produce a new image tag (and, via the build-arg digest, bust the layer
    // cache). `build_env` is sorted by the caller for a stable hash.
    for (k, v) in build_env {
        h.update(k.as_bytes());
        h.update([b'=']);
        h.update(v.as_bytes());
        h.update([0u8]);
    }
    hex_of(&h.finalize())
}

pub(crate) fn last8(uuid_str: &str) -> String {
    let n = uuid_str.len();
    uuid_str.chars().skip(n.saturating_sub(8)).collect()
}

pub(crate) fn service_alias(svc: ServiceId, env: EnvironmentId) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(svc.as_uuid().as_bytes());
    h.update(env.as_uuid().as_bytes());
    let digest = h.finalize();
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    format!("arx-svc-{}", &hex[..12])
}

pub(crate) fn service_hostname(
    project: &Project,
    service: &Service,
    environment: &Environment,
) -> String {
    format!("arx-{}-{}-{}", project.slug, service.slug, environment.slug)
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn restart_policy_from(s: &str) -> RestartPolicy {
    match s {
        "no" => RestartPolicy::No,
        "always" => RestartPolicy::Always,
        "on-failure" => RestartPolicy::OnFailure,
        _ => RestartPolicy::UnlessStopped,
    }
}

pub(crate) async fn deploy_docker_image(
    app: &AppState,
    ctx: DeployContext<'_>,
) -> ApiResult<Deployment> {
    let project_id: ProjectId = ctx.project.id;
    let service_id: ServiceId = ctx.service.id;
    let environment_id: EnvironmentId = ctx.environment.id;
    let project_slug = ctx.project.slug.clone();
    let env_slug = ctx.environment.slug.clone();
    let service_slug = ctx.service.slug.clone();
    let image = ctx.image;
    let extra_env = ctx.extra_env;
    let extra_mounts = ctx.extra_mounts;
    let existing_dep_id = ctx.existing_dep_id;

    let workspace_slug_for_label = ctx.workspace.slug.clone();

    let mut injected =
        crate::var_resolve::resolve_for_injection(app, project_id, service_id, environment_id)
            .await?;
    for (k, v) in &extra_env {
        injected.retain(|(ek, _)| ek != k);
        injected.push((k.clone(), v.clone()));
    }

    let snapshot = serde_json::Value::Object(
        injected
            .iter()
            .map(|(k, v)| {
                let masked = if k == "PORT" || k == "ARX_PORT" {
                    serde_json::Value::String(v.clone())
                } else {
                    serde_json::Value::String("***".into())
                };
                (k.clone(), masked)
            })
            .collect(),
    );

    let dep_id = match existing_dep_id {
        Some(id) => {
            sqlx::query(
                "UPDATE deployments SET image_ref = ?, variables_snapshot = ? WHERE id = ?",
            )
            .bind(&image)
            .bind(serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".into()))
            .bind(id.as_uuid().to_string())
            .execute(&app.db)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
            id
        }
        None => {
            deployments::create_pending(
                &app.db,
                service_id,
                environment_id,
                Some(&image),
                None,
                &snapshot,
            )
            .await?
        }
    };

    deployments::update_status(
        &app.db,
        dep_id,
        DeploymentStatus::Deploying,
        None,
        None,
        false,
    )
    .await?;

    let network_name = "arx".to_string();
    if let Err(e) = app.docker.ensure_network(&network_name).await {
        let msg = format!("ensure network: {e}");
        let _ = deployments::update_status(
            &app.db,
            dep_id,
            DeploymentStatus::Failed,
            None,
            Some(&msg),
            true,
        )
        .await;
        return Err(ApiError::internal(msg));
    }

    let container_name = format!(
        "arx-{}-{}",
        last8(&service_id.as_uuid().to_string()),
        last8(&dep_id.as_uuid().to_string())
    );

    let alias = service_alias(service_id, environment_id);
    let hostname = service_hostname(ctx.project, ctx.service, ctx.environment);

    // Prefer PORT over ARX_PORT, matching the precedence the traefik rewrite
    // uses when reading variables_snapshot, so the container port and the
    // routed port never disagree when both vars are set.
    let port_str = injected
        .iter()
        .find(|(k, _)| k == "PORT")
        .or_else(|| injected.iter().find(|(k, _)| k == "ARX_PORT"))
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| "8080".to_string());
    let port: u16 = port_str.parse().unwrap_or(8080);

    let mut labels = HashMap::new();
    labels.insert(
        "arx.deployment_id".to_string(),
        dep_id.as_uuid().to_string(),
    );
    labels.insert(
        "arx.service_id".to_string(),
        service_id.as_uuid().to_string(),
    );

    let env_cfg = service_env::get(&app.db, service_id, environment_id)
        .await
        .unwrap_or_default();
    let resources = ResourceLimits {
        cpu_cores: env_cfg.cpu_limit,
        memory_mb: env_cfg.memory_limit_mb,
    };

    labels.insert(
        "arx.workspace".to_string(),
        workspace_slug_for_label.clone(),
    );
    labels.insert("arx.project".to_string(), project_slug.clone());
    labels.insert("arx.service".to_string(), service_slug.clone());
    labels.insert("arx.environment".to_string(), env_slug.clone());

    // Run the pre-deploy command (e.g. DB migrations) in a throwaway container
    // sharing the new image/env/network, before the real container goes live.
    if let Some(pre_cmd) = ctx
        .service
        .pre_deploy_command
        .as_deref()
        .filter(|c| !c.is_empty())
    {
        tracing::info!(deployment_id = %dep_id.as_uuid(), "running pre-deploy command");
        let mut cmd = tokio::process::Command::new("docker");
        cmd.arg("run")
            .arg("--rm")
            .arg("--network")
            .arg(&network_name);
        for (k, v) in &injected {
            cmd.arg("-e").arg(format!("{k}={v}"));
        }
        cmd.arg(&image).arg("sh").arg("-lc").arg(pre_cmd);
        let result = cmd.output().await;
        let output = match result {
            Ok(o) => o,
            Err(e) => {
                let msg = format!("pre-deploy command: {e}");
                let _ = deployments::update_status(
                    &app.db,
                    dep_id,
                    DeploymentStatus::Failed,
                    None,
                    Some(&msg),
                    true,
                )
                .await;
                return Err(ApiError::internal(msg));
            }
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = format!(
                "pre-deploy command failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            );
            warn!("{msg}");
            let _ = deployments::update_status(
                &app.db,
                dep_id,
                DeploymentStatus::Failed,
                None,
                Some(&msg),
                true,
            )
            .await;
            return Err(ApiError::bad_request(msg));
        }
    }

    let spec = ContainerSpec {
        image: image.clone(),
        name: container_name.clone(),
        env: injected,
        ports: vec![PortBinding {
            container_port: port,
            protocol: Protocol::Tcp,
        }],
        mounts: extra_mounts,
        resources,
        restart: restart_policy_from(&ctx.service.restart_policy),
        network: Some(network_name.clone()),
        network_aliases: vec![alias.clone(), hostname.clone()],
        labels,
    };

    let handle = match app.docker.run(&spec).await {
        Ok(h) => h,
        Err(e) => {
            let msg = format!("run container: {e}");
            error!("{msg}");
            let _ = deployments::update_status(
                &app.db,
                dep_id,
                DeploymentStatus::Failed,
                None,
                Some(&msg),
                true,
            )
            .await;
            return Err(ApiError::internal(msg));
        }
    };

    let timeout_seconds = env_cfg.healthcheck_timeout_seconds.max(1) as u64;
    tracing::info!(
        deployment_id = %dep_id.as_uuid(),
        port,
        timeout_seconds,
        "waiting for healthy"
    );
    let healthy = wait_healthy(
        &app.docker,
        &app.http,
        &handle,
        port,
        env_cfg
            .healthcheck_path
            .as_deref()
            .filter(|s| !s.is_empty()),
        Duration::from_secs(timeout_seconds),
    )
    .await;
    tracing::info!(deployment_id = %dep_id.as_uuid(), healthy, "healthcheck complete");
    if !healthy {
        let msg = format!("healthcheck failed after {timeout_seconds}s");
        warn!("{msg}");
        let _ = app.docker.stop_and_remove(&handle).await;
        let _ = deployments::update_status(
            &app.db,
            dep_id,
            DeploymentStatus::Failed,
            Some(handle.as_str()),
            Some(&msg),
            true,
        )
        .await;
        return Err(ApiError::bad_request(msg));
    }

    let swap_lock = app.deploy_lock(service_id, environment_id);
    let _guard = swap_lock.lock().await;

    // Mark live, supersede the previous deployment, and swing traefik over.
    // If any step fails the new container is already running but unreachable,
    // so tear it down instead of leaking it (the previous version keeps serving).
    let swap: ApiResult<Vec<String>> = async {
        deployments::update_status(
            &app.db,
            dep_id,
            DeploymentStatus::Live,
            Some(handle.as_str()),
            None,
            false,
        )
        .await?;
        let prev =
            deployments::supersede_previous(&app.db, service_id, environment_id, dep_id).await?;
        svc_q::set_current_deployment(&app.db, service_id, environment_id, dep_id).await?;
        rewrite_traefik(app).await?;
        Ok(prev)
    }
    .await;

    let prev_containers = match swap {
        Ok(prev) => prev,
        Err(e) => {
            warn!(error = ?e, "deployment swap failed; tearing down new container");
            let _ = app.docker.stop_and_remove(&handle).await;
            let _ = deployments::update_status(
                &app.db,
                dep_id,
                DeploymentStatus::Failed,
                Some(handle.as_str()),
                Some("deployment swap failed"),
                true,
            )
            .await;
            drop(_guard);
            return Err(e);
        }
    };
    drop(_guard);

    for c in prev_containers {
        let h = arx_docker::ContainerHandle(c);
        if let Err(e) = app.docker.stop_and_remove(&h).await {
            warn!(error = %e, "failed to clean up superseded container");
        }
    }

    info!(deployment_id = %dep_id.as_uuid(), "deployment live");

    let dep = deployments::get(&app.db, dep_id).await?;
    Ok(dep)
}

async fn wait_healthy(
    engine: &arx_docker::DockerEngine,
    http: &reqwest::Client,
    handle: &arx_docker::ContainerHandle,
    port: u16,
    path: Option<&str>,
    timeout: Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;

    let container_ip = loop {
        if std::time::Instant::now() >= deadline {
            tracing::warn!("wait_healthy: deadline reached during container_ip lookup");
            return false;
        }
        match engine.status(handle).await {
            Ok(s) if s.is_running() => match engine.internal_address(handle).await {
                Ok(ip) => break ip,
                Err(e) => {
                    tracing::debug!(error = %e, "internal_address failed, retry");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            },
            Ok(arx_docker::ContainerStatus::Exited { code }) if code != 0 => {
                tracing::warn!(code, "wait_healthy: container exited non-zero");
                return false;
            }
            _ => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    };
    tracing::info!(container_ip = %container_ip, port, "probing TCP / HTTP");

    while std::time::Instant::now() < deadline {
        if let Ok(arx_docker::ContainerStatus::Exited { code }) = engine.status(handle).await {
            if code != 0 {
                return false;
            }
        }

        if let Some(p) = path {
            let url = format!("http://{container_ip}:{port}{p}");
            let req = http.get(&url).timeout(Duration::from_secs(3)).send().await;
            if let Ok(resp) = req {
                if resp.status().is_success() {
                    return true;
                }
            }
        } else {
            let addr = format!("{container_ip}:{port}");
            match tokio::time::timeout(
                Duration::from_secs(3),
                tokio::net::TcpStream::connect(&addr),
            )
            .await
            {
                Ok(Ok(_)) => return true,
                Ok(Err(e)) => tracing::warn!(addr = %addr, error = %e, "tcp probe: connect err"),
                Err(_) => tracing::warn!(addr = %addr, "tcp probe: timeout"),
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

pub async fn rewrite_traefik(app: &AppState) -> ApiResult<()> {
    let _guard = app.traefik_lock.lock().await;
    let all_domains = domains::list_all_active(&app.db).await?;
    let mut routes: Vec<Route> = Vec::new();
    let mut router_ids: Vec<String> = Vec::new();

    if let Ok(settings) = arx_db::queries::settings::get(&app.db).await {
        if let Some(admin) = settings.admin_domain {
            router_ids.push("arx".to_string());
            routes.push(Route {
                id: "arx".to_string(),
                host: admin,
                backend: arx_traefik::BackendTarget {
                    host: "arx".to_string(),
                    port: app.config.server.listen.port(),
                },
                tls: true,
            });
        }
    }

    for d in all_domains {
        let row = sqlx::query_as::<_, (Option<String>, String)>(
            "SELECT current_deployment_id, environment_id FROM service_env_configs
             WHERE service_id = ? AND environment_id = ?",
        )
        .bind(d.service_id.as_uuid().to_string())
        .bind(d.environment_id.as_uuid().to_string())
        .fetch_optional(&app.db)
        .await
        .map_err(|e| ApiError::internal(format!("sqlx: {e}")))?;

        let Some((Some(dep_id_str), _)) = row else {
            continue;
        };
        let dep_id =
            uuid::Uuid::parse_str(&dep_id_str).map_err(|e| ApiError::internal(e.to_string()))?;
        let dep = deployments::get(&app.db, arx_core::ids::DeploymentId::from_uuid(dep_id)).await?;

        let host = service_alias(d.service_id, d.environment_id);
        let port: u16 = dep
            .variables_snapshot
            .as_object()
            .and_then(|m| m.get("PORT").or_else(|| m.get("ARX_PORT")))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(8080);

        let id = format!(
            "svc-{}-env-{}",
            d.service_id.as_uuid().simple(),
            d.environment_id.as_uuid().simple()
        );
        router_ids.push(id.clone());
        routes.push(Route {
            id,
            host: d.hostname,
            backend: BackendTarget { host, port },
            tls: true,
        });
    }

    if let Err(e) = app.traefik.write_routes(&routes) {
        return Err(ApiError::internal(format!("traefik write: {e}")));
    }

    let id_refs: Vec<&str> = router_ids.iter().map(|s| s.as_str()).collect();
    let confirm_ms = if id_refs.is_empty() { 0 } else { 1500 };
    if confirm_ms > 0 {
        if let Err(e) = app
            .traefik
            .confirm(&id_refs, Duration::from_millis(confirm_ms))
            .await
        {
            warn!(error = %e, "traefik confirm failed (this is ok if traefik is not running locally)");
        }
    }
    Ok(())
}
