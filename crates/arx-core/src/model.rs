use crate::ids::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub github_login: String,
    pub github_user_id: i64,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Member,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub slug: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub id: MemberId,
    pub workspace_id: WorkspaceId,
    pub user_id: UserId,
    pub role: Role,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub slug: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub id: EnvironmentId,
    pub project_id: ProjectId,
    pub slug: String,
    pub name: String,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    GitSource,
    DockerImage,
    DbTemplate,
}

impl ServiceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ServiceKind::GitSource => "git_source",
            ServiceKind::DockerImage => "docker_image",
            ServiceKind::DbTemplate => "db_template",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbTemplate {
    Postgres,
    Mysql,
    Mongodb,
    Redis,
}

impl DbTemplate {
    pub fn as_str(&self) -> &'static str {
        match self {
            DbTemplate::Postgres => "postgres",
            DbTemplate::Mysql => "mysql",
            DbTemplate::Mongodb => "mongodb",
            DbTemplate::Redis => "redis",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub id: ServiceId,
    pub project_id: ProjectId,
    pub slug: String,
    pub name: String,
    pub kind: ServiceKind,

    pub source: ServiceSource,
    /// `None` = use the auto-detected stack's default.
    pub build_command: Option<String>,
    pub start_command: Option<String>,
    /// One-off command run in a throwaway container before the new container
    /// goes live (e.g. DB migrations). `None` = skip.
    pub pre_deploy_command: Option<String>,
    /// Docker restart policy: "no" | "unless-stopped" | "always" | "on-failure".
    pub restart_policy: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceSource {
    GitSource {
        github_repo: String,
        branch: String,

        dockerfile: Option<String>,
        root_directory: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        watch_paths: Option<Vec<String>>,
    },
    DockerImage {
        image: String,
        registry_credentials_id: Option<String>,
    },
    DbTemplate {
        template: DbTemplate,
        version: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HealthcheckMode {
    #[default]
    Tcp,
    Http,
    None,
}

impl HealthcheckMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthcheckMode::Tcp => "tcp",
            HealthcheckMode::Http => "http",
            HealthcheckMode::None => "none",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "tcp" => Some(HealthcheckMode::Tcp),
            "http" => Some(HealthcheckMode::Http),
            "none" => Some(HealthcheckMode::None),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEnvConfig {
    pub service_id: ServiceId,
    pub environment_id: EnvironmentId,
    pub cpu_limit: Option<f64>,
    pub memory_limit_mb: Option<i64>,
    pub healthcheck_mode: HealthcheckMode,
    pub healthcheck_path: Option<String>,
    pub healthcheck_timeout_seconds: i32,
    pub current_deployment_id: Option<DeploymentId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Pending,
    Building,
    Deploying,
    Live,
    Failed,
    Superseded,
    Rolledback,
}

impl DeploymentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeploymentStatus::Pending => "pending",
            DeploymentStatus::Building => "building",
            DeploymentStatus::Deploying => "deploying",
            DeploymentStatus::Live => "live",
            DeploymentStatus::Failed => "failed",
            DeploymentStatus::Superseded => "superseded",
            DeploymentStatus::Rolledback => "rolledback",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deployment {
    pub id: DeploymentId,
    pub service_id: ServiceId,
    pub environment_id: EnvironmentId,
    pub status: DeploymentStatus,
    pub image_ref: Option<String>,
    pub commit_sha: Option<String>,
    pub variables_snapshot: serde_json::Value,
    pub container_id: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub id: VariableId,
    pub service_id: ServiceId,
    pub environment_id: EnvironmentId,
    pub key: String,

    pub value_ciphertext: Vec<u8>,
    pub value_nonce: Vec<u8>,
    pub sealed: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Domain {
    pub id: DomainId,
    pub service_id: ServiceId,
    pub environment_id: EnvironmentId,
    pub hostname: String,
    pub verified: bool,
    pub cert_status: CertStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CertStatus {
    Pending,
    Issued,
    Failed,
}

impl CertStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CertStatus::Pending => "pending",
            CertStatus::Issued => "issued",
            CertStatus::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEvent {
    pub id: WebhookEventId,
    pub source: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub processed: bool,
    pub error: Option<String>,
    pub received_at: DateTime<Utc>,
}

/// What triggered a deployment, so the right outgoing-webhook event type can be
/// emitted (a rollback and a restart both produce a fresh `Live` deployment but
/// are distinct events to subscribers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeployTrigger {
    #[default]
    Deploy,
    Restart,
    Rollback,
}

/// A user-registered outgoing-webhook endpoint. `secret_ct`/`secret_nonce` hold
/// an encrypted credential JSON blob whose shape is interpreted by the transport
/// selected by `kind` (for `webhook`: `{"signing_secret": "..."}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEndpoint {
    pub id: WebhookEndpointId,
    pub workspace_id: WorkspaceId,
    /// `None` = all projects in the workspace.
    pub project_id: Option<ProjectId>,
    pub kind: String,
    pub url: String,
    pub config: serde_json::Value,
    pub secret_ct: Vec<u8>,
    pub secret_nonce: Vec<u8>,
    /// Subscribed event types, or `["*"]` for all.
    pub events: Vec<String>,
    pub active: bool,
    pub consecutive_failures: i64,
    pub first_failure_at: Option<DateTime<Utc>>,
    pub disabled_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    InFlight,
    Success,
    Failed,
}

impl DeliveryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeliveryStatus::Pending => "pending",
            DeliveryStatus::InFlight => "in_flight",
            DeliveryStatus::Success => "success",
            DeliveryStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(DeliveryStatus::Pending),
            "in_flight" => Some(DeliveryStatus::InFlight),
            "success" => Some(DeliveryStatus::Success),
            "failed" => Some(DeliveryStatus::Failed),
            _ => None,
        }
    }
}

/// A single attempt-bearing delivery record. `id` doubles as the stable
/// `X-Arx-Delivery` header value across retries (so receivers can deduplicate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookDelivery {
    pub id: WebhookDeliveryId,
    pub endpoint_id: WebhookEndpointId,
    pub event_id: String,
    pub event_type: String,
    pub payload: String,
    pub status: DeliveryStatus,
    pub attempts: i64,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub lease_until: Option<DateTime<Utc>>,
    pub response_status: Option<i64>,
    pub response_size: Option<i64>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub exhausted_at: Option<DateTime<Utc>>,
}
