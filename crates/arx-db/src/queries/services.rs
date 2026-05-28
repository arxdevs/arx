use super::{RowExt, map_sqlx};
use arx_core::ids::{EnvironmentId, ProjectId, ServiceId, WorkspaceId};
use arx_core::model::{Service, ServiceKind, ServiceSource};
use arx_core::{Error, Result};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};

const SELECT_COLS: &str =
    "id, project_id, slug, name, kind, source, build_command, start_command, created_at";

/// `Some(Some(s))` = set; `Some(None)` = clear; `None` = leave unchanged.
#[derive(Debug, Clone, Default)]
pub struct ServicePatch {
    pub name: Option<String>,
    pub build_command: Option<Option<String>>,
    pub start_command: Option<Option<String>>,
}

impl ServicePatch {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.build_command.is_none() && self.start_command.is_none()
    }
}

pub async fn create(
    pool: &SqlitePool,
    project_id: ProjectId,
    slug: &str,
    name: &str,
    source: &ServiceSource,
    build_command: Option<&str>,
    start_command: Option<&str>,
) -> Result<Service> {
    arx_core::slug::validate("service slug", slug)?;
    let id = ServiceId::new();
    let now = Utc::now();
    let kind = match source {
        ServiceSource::GitSource { .. } => ServiceKind::GitSource,
        ServiceSource::DockerImage { .. } => ServiceKind::DockerImage,
        ServiceSource::DbTemplate { .. } => ServiceKind::DbTemplate,
    };
    let source_json = serde_json::to_string(source).map_err(|e| Error::Internal(e.to_string()))?;

    let mut tx = pool.begin().await.map_err(map_sqlx)?;

    sqlx::query(
        "INSERT INTO services
         (id, project_id, slug, name, kind, source, build_command, start_command, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.as_uuid().to_string())
    .bind(project_id.as_uuid().to_string())
    .bind(slug)
    .bind(name)
    .bind(kind.as_str())
    .bind(&source_json)
    .bind(build_command)
    .bind(start_command)
    .bind(now.to_rfc3339())
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx)?;

    let env_rows = sqlx::query("SELECT id FROM environments WHERE project_id = ?")
        .bind(project_id.as_uuid().to_string())
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx)?;
    for env_row in env_rows {
        let env_id: String = env_row.try_get("id").map_err(map_sqlx)?;
        sqlx::query(
            "INSERT INTO service_env_configs
             (service_id, environment_id, cpu_limit, memory_limit_mb,
              healthcheck_path, healthcheck_timeout_seconds, current_deployment_id)
             VALUES (?, ?, NULL, NULL, NULL, 60, NULL)",
        )
        .bind(id.as_uuid().to_string())
        .bind(env_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
    }

    tx.commit().await.map_err(map_sqlx)?;

    Ok(Service {
        id,
        project_id,
        slug: slug.into(),
        name: name.into(),
        kind,
        source: source.clone(),
        build_command: build_command.map(String::from),
        start_command: start_command.map(String::from),
        created_at: now,
    })
}

pub async fn get_by_id(pool: &SqlitePool, id: ServiceId) -> Result<Service> {
    let q = format!("SELECT {SELECT_COLS} FROM services WHERE id = ?");
    let row = sqlx::query(&q)
        .bind(id.as_uuid().to_string())
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx)?;
    parse(&row.ok_or(Error::NotFound)?)
}

pub async fn get_by_slug(pool: &SqlitePool, project_id: ProjectId, slug: &str) -> Result<Service> {
    let q = format!("SELECT {SELECT_COLS} FROM services WHERE project_id = ? AND slug = ?");
    let row = sqlx::query(&q)
        .bind(project_id.as_uuid().to_string())
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx)?;
    parse(&row.ok_or(Error::NotFound)?)
}

pub async fn all_ids(pool: &SqlitePool) -> Result<Vec<ServiceId>> {
    let rows = sqlx::query("SELECT id FROM services")
        .fetch_all(pool)
        .await
        .map_err(map_sqlx)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(row.try_id::<ServiceId>("id")?);
    }
    Ok(out)
}

pub async fn list_in_project(pool: &SqlitePool, project_id: ProjectId) -> Result<Vec<Service>> {
    let q =
        format!("SELECT {SELECT_COLS} FROM services WHERE project_id = ? ORDER BY created_at ASC");
    let rows = sqlx::query(&q)
        .bind(project_id.as_uuid().to_string())
        .fetch_all(pool)
        .await
        .map_err(map_sqlx)?;
    rows.iter().map(parse).collect()
}

pub async fn update(pool: &SqlitePool, id: ServiceId, patch: &ServicePatch) -> Result<()> {
    if patch.is_empty() {
        return Ok(());
    }
    let mut sets: Vec<&'static str> = Vec::new();
    if patch.name.is_some() {
        sets.push("name = ?");
    }
    if patch.build_command.is_some() {
        sets.push("build_command = ?");
    }
    if patch.start_command.is_some() {
        sets.push("start_command = ?");
    }
    let sql = format!("UPDATE services SET {} WHERE id = ?", sets.join(", "));
    let mut q = sqlx::query(&sql);
    if let Some(name) = &patch.name {
        q = q.bind(name);
    }
    if let Some(bc) = &patch.build_command {
        q = q.bind(bc.as_deref());
    }
    if let Some(sc) = &patch.start_command {
        q = q.bind(sc.as_deref());
    }
    q.bind(id.as_uuid().to_string())
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
    Ok(())
}

pub async fn find_git_targets(
    pool: &SqlitePool,
    github_repo: &str,
    branch: &str,
) -> Result<Vec<GitTarget>> {
    let rows = sqlx::query(
        "SELECT
            s.id AS service_id,
            s.slug AS service_slug,
            p.id AS project_id,
            p.slug AS project_slug,
            p.workspace_id AS workspace_id,
            w.slug AS workspace_slug,
            e.id AS env_id,
            e.slug AS env_slug,
            s.source AS source
         FROM services s
         JOIN projects p ON p.id = s.project_id
         JOIN workspaces w ON w.id = p.workspace_id
         JOIN environments e ON e.project_id = p.id
         WHERE s.kind = 'git_source'
           AND json_extract(s.source, '$.github_repo') = ?
           AND json_extract(s.source, '$.branch') = ?",
    )
    .bind(github_repo)
    .bind(branch)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let source_json: String = r.try_get("source").map_err(map_sqlx)?;
        let parsed: serde_json::Value = serde_json::from_str(&source_json).unwrap_or_default();
        let root_directory = parsed
            .get("root_directory")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let watch_paths = parsed
            .get("watch_paths")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>()
            });
        out.push(GitTarget {
            workspace_id: r.try_id::<WorkspaceId>("workspace_id")?,
            workspace_slug: r.try_get("workspace_slug").map_err(map_sqlx)?,
            project_id: r.try_id::<ProjectId>("project_id")?,
            project_slug: r.try_get("project_slug").map_err(map_sqlx)?,
            service_id: r.try_id::<ServiceId>("service_id")?,
            service_slug: r.try_get("service_slug").map_err(map_sqlx)?,
            environment_id: r.try_id::<EnvironmentId>("env_id")?,
            environment_slug: r.try_get("env_slug").map_err(map_sqlx)?,
            root_directory,
            watch_paths,
        });
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct GitTarget {
    pub workspace_id: arx_core::ids::WorkspaceId,
    pub workspace_slug: String,
    pub project_id: ProjectId,
    pub project_slug: String,
    pub service_id: ServiceId,
    pub service_slug: String,
    pub environment_id: EnvironmentId,
    pub environment_slug: String,
    pub root_directory: Option<String>,
    pub watch_paths: Option<Vec<String>>,
}

pub async fn set_current_deployment(
    pool: &SqlitePool,
    service_id: ServiceId,
    environment_id: EnvironmentId,
    deployment_id: arx_core::ids::DeploymentId,
) -> Result<()> {
    sqlx::query(
        "UPDATE service_env_configs SET current_deployment_id = ?
         WHERE service_id = ? AND environment_id = ?",
    )
    .bind(deployment_id.as_uuid().to_string())
    .bind(service_id.as_uuid().to_string())
    .bind(environment_id.as_uuid().to_string())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

fn parse(row: &sqlx::sqlite::SqliteRow) -> Result<Service> {
    let slug: String = row.try_get("slug").map_err(map_sqlx)?;
    let name: String = row.try_get("name").map_err(map_sqlx)?;
    let kind_str: String = row.try_get("kind").map_err(map_sqlx)?;
    let source_json: String = row.try_get("source").map_err(map_sqlx)?;
    let build_command: Option<String> = row.try_get("build_command").map_err(map_sqlx)?;
    let start_command: Option<String> = row.try_get("start_command").map_err(map_sqlx)?;
    let created: String = row.try_get("created_at").map_err(map_sqlx)?;
    let kind = match kind_str.as_str() {
        "git_source" => ServiceKind::GitSource,
        "docker_image" => ServiceKind::DockerImage,
        "db_template" => ServiceKind::DbTemplate,
        other => return Err(Error::Internal(format!("unknown service kind: {other}"))),
    };
    let source: ServiceSource =
        serde_json::from_str(&source_json).map_err(|e| Error::Internal(e.to_string()))?;
    Ok(Service {
        id: row.try_id::<ServiceId>("id")?,
        project_id: row.try_id::<ProjectId>("project_id")?,
        slug,
        name,
        kind,
        source,
        build_command,
        start_command,
        created_at: DateTime::parse_from_rfc3339(&created)
            .map_err(|e| Error::Internal(e.to_string()))?
            .with_timezone(&Utc),
    })
}
