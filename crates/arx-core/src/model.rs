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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEnvConfig {
    pub service_id: ServiceId,
    pub environment_id: EnvironmentId,
    pub cpu_limit: Option<f64>,
    pub memory_limit_mb: Option<i64>,
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
