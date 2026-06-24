//! Outgoing webhooks: emit lifecycle events to user-registered endpoints,
//! deliver them with signing + retries, and prune old delivery records.
//!
//! Emission is fire-and-forget: it only INSERTs `pending` delivery rows (never
//! inline network I/O) and never propagates its own errors into the deploy or
//! backup path. A background worker performs the actual signed POSTs with
//! exponential backoff, dead-lettering, and per-endpoint auto-disable.

pub mod ssrf;
pub mod transport;
pub mod worker;

use crate::state::AppState;
use arx_core::ids::{ProjectId, WorkspaceId};
use arx_core::model::DeployTrigger;
use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

pub use worker::{spawn_pruner, spawn_worker};

/// The canonical event envelope sent to `kind=webhook` endpoints. Slack/Discord
/// transports would render their own body from the same typed event later.
#[derive(Debug, Clone, Serialize)]
pub struct EventEnvelope {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub created_at: String,
    pub workspace: String,
    pub data: serde_json::Value,
}

/// Builds a hardened HTTP client for outbound delivery: no redirects (3xx is a
/// failure, not a bypass), the SSRF [`ssrf::GuardedResolver`], a per-request
/// timeout, and TLS verification left on. A single constructor so the worker
/// and any test/redeliver path cannot accidentally use a default client.
pub fn build_delivery_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .dns_resolver(std::sync::Arc::new(ssrf::GuardedResolver::new()))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Maps a deploy trigger + outcome to the emitted event type.
fn deploy_event_type(trigger: DeployTrigger, succeeded: bool, started: bool) -> &'static str {
    if started {
        return match trigger {
            DeployTrigger::Deploy => "deployment.started",
            DeployTrigger::Restart => "service.restarting",
            DeployTrigger::Rollback => "deployment.rolling_back",
        };
    }
    match (trigger, succeeded) {
        (DeployTrigger::Deploy, true) => "deployment.succeeded",
        (DeployTrigger::Deploy, false) => "deployment.failed",
        (DeployTrigger::Restart, true) => "service.restarted",
        (DeployTrigger::Restart, false) => "service.restart_failed",
        (DeployTrigger::Rollback, true) => "deployment.rolled_back",
        (DeployTrigger::Rollback, false) => "deployment.rollback_failed",
    }
}

/// Context for a deploy-related emission. Slugs only — never raw error text or
/// secret-bearing strings.
pub struct DeployEventCtx {
    pub workspace_id: WorkspaceId,
    pub workspace_slug: String,
    pub project_id: ProjectId,
    pub project_slug: String,
    pub service_slug: String,
    pub environment_slug: String,
}

/// Emits the `started` event for a deploy/restart/rollback. Fire-and-forget.
pub async fn emit_deploy_started(app: &AppState, ctx: &DeployEventCtx, trigger: DeployTrigger) {
    let event_type = deploy_event_type(trigger, false, true);
    let data = serde_json::json!({
        "project": ctx.project_slug,
        "service": ctx.service_slug,
        "environment": ctx.environment_slug,
    });
    emit(app, ctx.workspace_id, Some(ctx.project_id), &ctx.workspace_slug, event_type, data).await;
}

/// Emits the terminal event for a deploy/restart/rollback based on its result.
/// `reason` is a coarse, secret-free classification on failure (never raw
/// stderr). Fire-and-forget.
pub async fn emit_deploy_terminal(
    app: &AppState,
    ctx: &DeployEventCtx,
    trigger: DeployTrigger,
    deployment_id: Option<&str>,
    succeeded: bool,
    reason: Option<&str>,
) {
    let event_type = deploy_event_type(trigger, succeeded, false);
    let data = serde_json::json!({
        "project": ctx.project_slug,
        "service": ctx.service_slug,
        "environment": ctx.environment_slug,
        "deployment_id": deployment_id,
        "status": if succeeded { "live" } else { "failed" },
        "reason": reason,
    });
    emit(app, ctx.workspace_id, Some(ctx.project_id), &ctx.workspace_slug, event_type, data).await;
}

/// Emits a backup event for a service, resolving its project + workspace for
/// the payload. Fire-and-forget (lookup failures are swallowed).
pub async fn emit_backup_for_service(
    app: &AppState,
    service: &arx_core::model::Service,
    succeeded: bool,
    reason: Option<&str>,
) {
    let project = match arx_db::queries::projects::get_by_id(&app.db, service.project_id).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "webhook emit_backup: project lookup failed");
            return;
        }
    };
    let workspace = match arx_db::queries::workspaces::get_by_id(&app.db, project.workspace_id).await
    {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(error = %e, "webhook emit_backup: workspace lookup failed");
            return;
        }
    };
    let event_type = if succeeded {
        "backup.succeeded"
    } else {
        "backup.failed"
    };
    let data = serde_json::json!({
        "project": project.slug,
        "service": service.slug,
        "status": if succeeded { "ok" } else { "failed" },
        "reason": reason,
    });
    emit(
        app,
        workspace.id,
        Some(project.id),
        &workspace.slug,
        event_type,
        data,
    )
    .await;
}

/// Queues a `test` event for a single endpoint (used by the /test API). Returns
/// the created delivery id, or None if queuing failed.
pub async fn emit_test(
    app: &AppState,
    endpoint_id: arx_core::ids::WebhookEndpointId,
    workspace_slug: &str,
) -> Option<arx_core::ids::WebhookDeliveryId> {
    let event_id = format!("evt_{}", Uuid::now_v7());
    let envelope = EventEnvelope {
        id: event_id.clone(),
        event_type: "test".to_string(),
        created_at: Utc::now().to_rfc3339(),
        workspace: workspace_slug.to_string(),
        data: serde_json::json!({ "message": "arx outgoing webhook test event" }),
    };
    let body = serde_json::to_string(&envelope).ok()?;
    match arx_db::queries::webhooks::create_pending(
        &app.db,
        endpoint_id,
        &event_id,
        "test",
        &body,
    )
    .await
    {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!(error = %e, "failed to queue webhook test delivery");
            None
        }
    }
}

/// Core emission: find matching active endpoints and INSERT a pending delivery
/// row per endpoint. Never blocks the caller on network and never returns an
/// error (logs and continues) so deploy/backup paths are unaffected.
async fn emit(
    app: &AppState,
    workspace_id: WorkspaceId,
    project_id: Option<ProjectId>,
    workspace_slug: &str,
    event_type: &str,
    data: serde_json::Value,
) {
    let endpoints = match arx_db::queries::webhooks::list_active_for_event(
        &app.db,
        workspace_id,
        project_id,
        event_type,
    )
    .await
    {
        Ok(eps) => eps,
        Err(e) => {
            tracing::warn!(error = %e, event = event_type, "webhook emit: endpoint lookup failed");
            return;
        }
    };
    if endpoints.is_empty() {
        return;
    }

    let event_id = format!("evt_{}", Uuid::now_v7());
    let envelope = EventEnvelope {
        id: event_id.clone(),
        event_type: event_type.to_string(),
        created_at: Utc::now().to_rfc3339(),
        workspace: workspace_slug.to_string(),
        data,
    };
    let body = match serde_json::to_string(&envelope) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(error = %e, "webhook emit: serialize failed");
            return;
        }
    };

    for ep in endpoints {
        if let Err(e) =
            arx_db::queries::webhooks::create_pending(&app.db, ep.id, &event_id, event_type, &body)
                .await
        {
            tracing::warn!(error = %e, endpoint = %ep.id.as_uuid(), "webhook emit: enqueue failed");
        }
    }
}
