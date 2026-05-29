use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use arx_core::ids::{ProjectId, UserId, WorkspaceId};
use arx_core::model::{Environment, Service, ServiceSource};
use arx_docker::{ContainerEngine, ContainerHandle};
use sqlx::Row;
use tracing::warn;

#[derive(Debug, Clone, Copy, Default)]
pub struct DeleteOpts {
    pub force: bool,
    pub with_data: bool,
}

pub async fn delete_workspace(
    app: &AppState,
    actor: UserId,
    ws_id: WorkspaceId,
    ws_slug: &str,
    opts: DeleteOpts,
) -> ApiResult<()> {
    let projects = arx_db::queries::projects::list_in_workspace(&app.db, ws_id).await?;

    if !projects.is_empty() && !opts.force {
        return Err(ApiError::already_exists(format!(
            "workspace `{ws_slug}` has {} project(s); use --force to cascade",
            projects.len()
        )));
    }

    for p in &projects {
        delete_project(app, actor, p.id, &p.slug, opts).await?;
    }

    sqlx::query("DELETE FROM workspaces WHERE id = ?")
        .bind(ws_id.as_uuid().to_string())
        .execute(&app.db)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let _ = arx_db::queries::audit::write(
        &app.db,
        Some(actor),
        "workspace.delete",
        &format!("workspace:{ws_slug}"),
        serde_json::json!({"force": opts.force, "with_data": opts.with_data}),
    )
    .await;

    Ok(())
}

pub async fn delete_project(
    app: &AppState,
    actor: UserId,
    project_id: ProjectId,
    project_slug: &str,
    opts: DeleteOpts,
) -> ApiResult<()> {
    let services = arx_db::queries::services::list_in_project(&app.db, project_id).await?;

    if !services.is_empty() && !opts.force {
        return Err(ApiError::already_exists(format!(
            "project `{project_slug}` has {} service(s); use --force to cascade",
            services.len()
        )));
    }

    for s in &services {
        delete_service(app, actor, s, opts).await?;
    }

    sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(project_id.as_uuid().to_string())
        .execute(&app.db)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let _ = arx_db::queries::audit::write(
        &app.db,
        Some(actor),
        "project.delete",
        &format!("project:{project_slug}"),
        serde_json::json!({"force": opts.force, "with_data": opts.with_data}),
    )
    .await;

    Ok(())
}

pub async fn delete_service(
    app: &AppState,
    actor: UserId,
    service: &Service,
    opts: DeleteOpts,
) -> ApiResult<()> {
    let rows = sqlx::query(
        "SELECT DISTINCT container_id FROM deployments
         WHERE service_id = ? AND container_id IS NOT NULL",
    )
    .bind(service.id.as_uuid().to_string())
    .fetch_all(&app.db)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    for row in rows {
        if let Ok(Some(cid)) = row.try_get::<Option<String>, _>("container_id") {
            let handle = ContainerHandle(cid);
            if let Err(e) = app.docker.stop_and_remove(&handle).await {
                warn!(error = %e, "failed to remove container during cascade");
            }
        }
    }

    if matches!(service.source, ServiceSource::GitSource { .. }) {
        let repo_root = &app.config.paths.repos_dir;
        if let Ok(read) = std::fs::read_dir(repo_root) {
            let prefix = format!("{}-", service.id.as_uuid());
            for entry in read.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with(&prefix) {
                        let _ = std::fs::remove_dir_all(entry.path());
                    }
                }
            }
        }
    }

    if opts.with_data {
        let envs = arx_db::queries::environments::list_in_project(&app.db, service.project_id)
            .await
            .unwrap_or_default();
        for env in envs {
            let name = crate::db_template::volume_name(service, &env);
            if let Err(e) = app.docker.remove_volume(&name).await {
                warn!(error = %e, volume = %name, "failed to remove volume during cascade");
            }
        }

        let backup_dir = app
            .config
            .paths
            .backups_dir
            .join(service.id.as_uuid().to_string());
        let _ = std::fs::remove_dir_all(&backup_dir);
    }

    sqlx::query("DELETE FROM services WHERE id = ?")
        .bind(service.id.as_uuid().to_string())
        .execute(&app.db)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    crate::deploy::rewrite_traefik(app).await?;

    let _ = arx_db::queries::audit::write(
        &app.db,
        Some(actor),
        "service.delete",
        &format!("service:{}", service.slug),
        serde_json::json!({"force": opts.force, "with_data": opts.with_data, "kind": service.kind.as_str()}),
    )
    .await;

    Ok(())
}

pub async fn delete_service_by_slug(
    app: &AppState,
    actor: UserId,
    project_id: ProjectId,
    service_slug: &str,
    opts: DeleteOpts,
) -> ApiResult<()> {
    let s = arx_db::queries::services::get_by_slug(&app.db, project_id, service_slug).await?;
    delete_service(app, actor, &s, opts).await
}

pub async fn delete_environment(
    app: &AppState,
    actor: UserId,
    env: &Environment,
    opts: DeleteOpts,
) -> ApiResult<()> {
    if env.is_default {
        return Err(ApiError::bad_request(
            "cannot delete the default environment",
        ));
    }

    let active: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM deployments WHERE environment_id = ? AND status = 'live'",
    )
    .bind(env.id.as_uuid().to_string())
    .fetch_one(&app.db)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .try_get("n")
    .map_err(|e| ApiError::internal(e.to_string()))?;
    if active > 0 && !opts.force {
        return Err(ApiError::already_exists(format!(
            "environment `{}` has {active} live deployment(s); use --force to cascade",
            env.slug
        )));
    }

    // Tear down every container that belongs to this environment.
    let rows = sqlx::query(
        "SELECT DISTINCT container_id FROM deployments
         WHERE environment_id = ? AND container_id IS NOT NULL",
    )
    .bind(env.id.as_uuid().to_string())
    .fetch_all(&app.db)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    for row in rows {
        if let Ok(Some(cid)) = row.try_get::<Option<String>, _>("container_id") {
            let handle = ContainerHandle(cid);
            if let Err(e) = app.docker.stop_and_remove(&handle).await {
                warn!(error = %e, "failed to remove container during environment delete");
            }
        }
    }

    if opts.with_data {
        let services = arx_db::queries::services::list_in_project(&app.db, env.project_id).await?;
        for s in &services {
            let name = crate::db_template::volume_name(s, env);
            if let Err(e) = app.docker.remove_volume(&name).await {
                warn!(error = %e, volume = %name, "failed to remove volume during environment delete");
            }
        }
    }

    // ON DELETE CASCADE removes this env's deployments, variables, domains, and configs.
    sqlx::query("DELETE FROM environments WHERE id = ?")
        .bind(env.id.as_uuid().to_string())
        .execute(&app.db)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    crate::deploy::rewrite_traefik(app).await?;

    let _ = arx_db::queries::audit::write(
        &app.db,
        Some(actor),
        "environment.delete",
        &format!("environment:{}", env.slug),
        serde_json::json!({"force": opts.force, "with_data": opts.with_data}),
    )
    .await;

    Ok(())
}

pub async fn delete_environment_by_slug(
    app: &AppState,
    actor: UserId,
    project_id: ProjectId,
    env_slug: &str,
    opts: DeleteOpts,
) -> ApiResult<()> {
    let e = arx_db::queries::environments::get_by_slug(&app.db, project_id, env_slug).await?;
    delete_environment(app, actor, &e, opts).await
}
