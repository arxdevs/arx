use crate::error::ApiResult;
use crate::state::AppState;
use arx_docker::ContainerEngine;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use tracing::warn;

const MANAGED_LABEL: &str = "arx.managed";
const SERVICE_ID_LABEL: &str = "arx.service-id";
const ENVIRONMENT_ID_LABEL: &str = "arx.environment-id";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    InUse,
    Orphan,
    InUseByUnknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct VolumeReport {
    pub name: String,
    pub service_id: Option<String>,
    pub environment_id: Option<String>,
    pub classification: Classification,
}

#[derive(Debug, Clone, Serialize)]
pub struct PruneResult {
    pub removed: Vec<String>,
    pub skipped: Vec<SkippedVolume>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedVolume {
    pub name: String,
    pub reason: String,
}

pub async fn list(app: &AppState) -> ApiResult<Vec<VolumeReport>> {
    let filter = HashMap::from([(MANAGED_LABEL.to_string(), "true".to_string())]);
    let volumes = app
        .docker
        .list_volumes(&filter)
        .await
        .map_err(|e| crate::error::ApiError::internal(e.to_string()))?;

    let known: HashSet<String> = arx_db::queries::services::all_ids(&app.db)
        .await?
        .into_iter()
        .map(|id| id.as_uuid().to_string())
        .collect();

    let mut out = Vec::with_capacity(volumes.len());
    for v in volumes {
        let service_id = v.labels.get(SERVICE_ID_LABEL).cloned();
        let environment_id = v.labels.get(ENVIRONMENT_ID_LABEL).cloned();
        let matched = service_id
            .as_ref()
            .map(|s| known.contains(s))
            .unwrap_or(false);
        let classification = match (v.ref_count > 0, matched) {
            (true, true) => Classification::InUse,
            (true, false) => Classification::InUseByUnknown,
            (false, _) => Classification::Orphan,
        };
        out.push(VolumeReport {
            name: v.name,
            service_id,
            environment_id,
            classification,
        });
    }
    Ok(out)
}

pub async fn prune(app: &AppState, dry_run: bool) -> ApiResult<PruneResult> {
    let reports = list(app).await?;
    let mut removed = Vec::new();
    let mut skipped = Vec::new();
    for r in reports {
        match r.classification {
            Classification::Orphan => {
                if dry_run {
                    removed.push(r.name);
                } else if let Err(e) = app.docker.remove_volume(&r.name).await {
                    warn!(error = %e, volume = %r.name, "prune: remove failed");
                    skipped.push(SkippedVolume {
                        name: r.name,
                        reason: format!("remove failed: {e}"),
                    });
                } else {
                    removed.push(r.name);
                }
            }
            Classification::InUse => skipped.push(SkippedVolume {
                name: r.name,
                reason: "in use by known service".into(),
            }),
            Classification::InUseByUnknown => skipped.push(SkippedVolume {
                name: r.name,
                reason: "in use by unknown container".into(),
            }),
        }
    }
    Ok(PruneResult {
        removed,
        skipped,
        dry_run,
    })
}
