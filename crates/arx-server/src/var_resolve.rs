use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use arx_core::ids::{EnvironmentId, ProjectId, ServiceId};
use arx_core::refs::{ResolveError, find_references};
use std::collections::{HashMap, HashSet};

const MAX_DEPTH: u32 = 32;

pub async fn resolve_for_injection(
    app: &AppState,
    project_id: ProjectId,
    service_id: ServiceId,
    environment_id: EnvironmentId,
) -> ApiResult<Vec<(String, String)>> {
    let raw = arx_db::queries::variables::for_injection(
        &app.db,
        &app.master_key,
        service_id,
        environment_id,
    )
    .await?;

    let mut cache: HashMap<(String, String), String> = HashMap::new();

    let mut out = Vec::with_capacity(raw.len());
    for (k, v) in raw {
        let resolved = resolve_value(app, project_id, environment_id, &v, &mut cache)
            .await
            .map_err(map_err)?;
        out.push((k, resolved));
    }
    Ok(out)
}

async fn resolve_value(
    app: &AppState,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    value: &str,
    cache: &mut HashMap<(String, String), String>,
) -> Result<String, ResolveError> {
    let mut visited: HashSet<(String, String)> = HashSet::new();
    resolve_inner(
        app,
        project_id,
        environment_id,
        value,
        &mut visited,
        cache,
        0,
    )
    .await
}

fn resolve_inner<'a>(
    app: &'a AppState,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    value: &'a str,
    visited: &'a mut HashSet<(String, String)>,
    cache: &'a mut HashMap<(String, String), String>,
    depth: u32,
) -> futures::future::BoxFuture<'a, Result<String, ResolveError>> {
    Box::pin(async move {
        if depth > MAX_DEPTH {
            return Err(ResolveError::DepthExceeded { max: MAX_DEPTH });
        }

        let refs = find_references(value);
        if refs.is_empty() {
            return Ok(value.to_string());
        }

        let mut out = String::with_capacity(value.len());
        let mut last = 0;
        for r in refs {
            let (start, end) = r.span;
            out.push_str(&value[last..start]);

            let key = (r.service_slug.clone(), r.variable_key.clone());

            if visited.contains(&key) {
                let mut path: Vec<String> =
                    visited.iter().map(|(s, v)| format!("{s}.{v}")).collect();
                path.push(format!("{}.{}", r.service_slug, r.variable_key));
                return Err(ResolveError::Cycle {
                    path: path.join(" → "),
                });
            }

            if let Some(v) = cache.get(&key) {
                out.push_str(v);
                last = end;
                continue;
            }

            let target_svc =
                match arx_db::queries::services::get_by_slug(&app.db, project_id, &r.service_slug)
                    .await
                {
                    Ok(s) => s,
                    Err(_) => {
                        return Err(ResolveError::ServiceNotFound {
                            service: r.service_slug.clone(),
                        });
                    }
                };

            let target_vars = arx_db::queries::variables::for_injection(
                &app.db,
                &app.master_key,
                target_svc.id,
                environment_id,
            )
            .await
            .map_err(|e| ResolveError::LookupFailed {
                service: r.service_slug.clone(),
                detail: e.to_string(),
            })?;
            let Some((_, raw_value)) = target_vars.into_iter().find(|(k, _)| k == &r.variable_key)
            else {
                return Err(ResolveError::VariableNotFound {
                    service: r.service_slug,
                    var: r.variable_key,
                });
            };

            visited.insert(key.clone());
            let resolved = resolve_inner(
                app,
                project_id,
                environment_id,
                &raw_value,
                visited,
                cache,
                depth + 1,
            )
            .await?;
            visited.remove(&key);
            cache.insert(key, resolved.clone());

            out.push_str(&resolved);
            last = end;
        }
        out.push_str(&value[last..]);
        Ok(out)
    })
}

fn map_err(e: ResolveError) -> ApiError {
    tracing::warn!(error = %e, "variable reference resolution failed");
    match e {
        ResolveError::ServiceNotFound { service } => {
            ApiError::bad_request(format!("variable references unknown service `{service}`"))
        }
        ResolveError::VariableNotFound { service, .. } => ApiError::bad_request(format!(
            "variable references unknown variable in service `{service}`"
        )),
        ResolveError::Cycle { .. } => {
            ApiError::bad_request("variable references contain a cycle".to_string())
        }
        ResolveError::DepthExceeded { max } => {
            ApiError::bad_request(format!("variable reference depth exceeded ({max})"))
        }
        ResolveError::LookupFailed { .. } => {
            ApiError::internal("failed to resolve variable references".to_string())
        }
    }
}
