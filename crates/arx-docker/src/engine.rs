use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContainerHandle(pub String);

impl ContainerHandle {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerSpec {
    pub image: String,

    pub name: String,

    pub env: Vec<(String, String)>,

    pub ports: Vec<PortBinding>,

    pub mounts: Vec<Mount>,

    pub resources: ResourceLimits,

    pub restart: RestartPolicy,

    pub network: Option<String>,

    pub network_aliases: Vec<String>,

    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortBinding {
    pub container_port: u16,
    pub protocol: Protocol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Mount {
    Bind {
        host_path: String,
        container_path: String,
        read_only: bool,
    },
    NamedVolume {
        name: String,
        container_path: String,
        read_only: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeInfo {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub ref_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub cpu_cores: Option<f64>,

    pub memory_mb: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    No,
    #[default]
    UnlessStopped,
    Always,
    OnFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerStatus {
    Created,
    Running {
        restart_count: i64,
        started_at: Option<String>,
    },
    Restarting,
    Paused,
    Exited {
        code: i64,
    },
    Dead,
    Removing,
    Unknown,
}

impl ContainerStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, ContainerStatus::Running { .. })
    }
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub stream: LogStreamKind,
    pub line: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStreamKind {
    Stdout,
    Stderr,
}

pub type LogStream = BoxStream<'static, std::result::Result<LogLine, EngineError>>;

#[derive(Debug, Clone, Default)]
pub struct LogOptions {
    pub follow: bool,
    /// Last N lines; `None` = all.
    pub tail: Option<u32>,
    /// Only logs at or after this unix timestamp (seconds); `None` = from start.
    pub since: Option<i64>,
}

/// A live `docker exec` attachment: a byte stream of combined stdout/stderr and
/// a writer for the exec's stdin.
pub struct ExecSession {
    pub output: BoxStream<'static, std::result::Result<Vec<u8>, EngineError>>,
    pub input: std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("pull failed: {0}")]
    PullFailed(String),

    #[error("create failed: {0}")]
    CreateFailed(String),

    #[error("start failed: {0}")]
    StartFailed(String),

    #[error("io: {0}")]
    Io(String),

    #[error("docker: {0}")]
    Other(String),
}

#[async_trait]
pub trait ContainerEngine: Send + Sync + 'static {
    async fn pull_image(&self, image: &str) -> Result<(), EngineError>;

    async fn ensure_network(&self, name: &str) -> Result<(), EngineError>;

    async fn run(&self, spec: &ContainerSpec) -> Result<ContainerHandle, EngineError>;

    async fn stop_and_remove(&self, handle: &ContainerHandle) -> Result<(), EngineError>;

    async fn ensure_volume(
        &self,
        name: &str,
        labels: &HashMap<String, String>,
    ) -> Result<(), EngineError>;

    async fn remove_volume(&self, name: &str) -> Result<(), EngineError>;

    async fn list_volumes(
        &self,
        labels: &HashMap<String, String>,
    ) -> Result<Vec<VolumeInfo>, EngineError>;

    async fn status(&self, handle: &ContainerHandle) -> Result<ContainerStatus, EngineError>;

    async fn internal_address(&self, handle: &ContainerHandle) -> Result<String, EngineError>;

    async fn logs(
        &self,
        handle: &ContainerHandle,
        opts: LogOptions,
    ) -> Result<LogStream, EngineError>;
}
