use crate::engine::*;
use async_trait::async_trait;
use bollard::Docker;
use bollard::container::{
    Config, CreateContainerOptions, LogOutput, LogsOptions, RemoveContainerOptions,
    StopContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::{
    ContainerStateStatusEnum, EndpointSettings, HostConfig, Mount, MountTypeEnum,
    PortBinding as DockerPortBinding, RestartPolicy as DockerRestartPolicy, RestartPolicyNameEnum,
};
use bollard::network::CreateNetworkOptions;
use futures::StreamExt;
use std::collections::HashMap;
use tracing::{debug, info, warn};

const ARX_OWNED_LABEL: &str = "arx.managed";

pub struct DockerEngine {
    docker: Docker,
}

impl DockerEngine {
    pub fn connect_local() -> Result<Self, EngineError> {
        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| EngineError::Other(format!("connect docker: {e}")))?;
        Ok(Self { docker })
    }

    #[cfg(test)]
    pub fn from_docker(docker: Docker) -> Self {
        Self { docker }
    }
}

fn map_err(e: bollard::errors::Error) -> EngineError {
    use bollard::errors::Error::*;
    match e {
        DockerResponseServerError {
            status_code: 404, ..
        } => EngineError::NotFound(e.to_string()),
        DockerResponseServerError {
            status_code: 409, ..
        } => EngineError::Conflict(e.to_string()),
        other => EngineError::Other(other.to_string()),
    }
}

#[async_trait]
impl ContainerEngine for DockerEngine {
    async fn pull_image(&self, image: &str) -> Result<(), EngineError> {
        info!(image, "pulling image");
        let opts = CreateImageOptions {
            from_image: image.to_string(),
            ..Default::default()
        };
        let mut stream = self.docker.create_image(Some(opts), None, None);
        while let Some(item) = stream.next().await {
            match item {
                Ok(info) => {
                    if let Some(status) = info.status {
                        debug!(image, "pull progress: {status}");
                    }
                }
                Err(e) => return Err(EngineError::PullFailed(e.to_string())),
            }
        }
        Ok(())
    }

    async fn ensure_network(&self, name: &str) -> Result<(), EngineError> {
        let existing = self
            .docker
            .list_networks::<&str>(None)
            .await
            .map_err(map_err)?;
        if existing.iter().any(|n| n.name.as_deref() == Some(name)) {
            return Ok(());
        }
        let opts = CreateNetworkOptions::<String> {
            name: name.to_string(),
            driver: "bridge".to_string(),
            ..Default::default()
        };
        self.docker.create_network(opts).await.map_err(map_err)?;
        Ok(())
    }

    async fn run(&self, spec: &ContainerSpec) -> Result<ContainerHandle, EngineError> {
        if !spec.image.contains('@') || !spec.image.starts_with("sha256:") {
            if let Err(e) = self.pull_image(&spec.image).await {
                warn!(error = %e, "image pull failed, attempting to create container anyway");
            }
        }

        let mut port_bindings: HashMap<String, Option<Vec<DockerPortBinding>>> = HashMap::new();
        let mut exposed_ports: HashMap<String, HashMap<(), ()>> = HashMap::new();
        for p in &spec.ports {
            let proto = match p.protocol {
                Protocol::Tcp => "tcp",
                Protocol::Udp => "udp",
            };
            let key = format!("{}/{}", p.container_port, proto);

            port_bindings.insert(key.clone(), None);
            exposed_ports.insert(key, HashMap::new());
        }

        let mounts = spec
            .mounts
            .iter()
            .map(|m| Mount {
                target: Some(m.container_path.clone()),
                source: Some(m.host_path.clone()),
                typ: Some(MountTypeEnum::BIND),
                read_only: Some(m.read_only),
                ..Default::default()
            })
            .collect::<Vec<_>>();

        let restart = DockerRestartPolicy {
            name: Some(match spec.restart {
                RestartPolicy::No => RestartPolicyNameEnum::NO,
                RestartPolicy::UnlessStopped => RestartPolicyNameEnum::UNLESS_STOPPED,
                RestartPolicy::Always => RestartPolicyNameEnum::ALWAYS,
                RestartPolicy::OnFailure => RestartPolicyNameEnum::ON_FAILURE,
            }),
            maximum_retry_count: None,
        };

        let nano_cpus = spec
            .resources
            .cpu_cores
            .map(|c| (c * 1_000_000_000.0) as i64);
        let memory_bytes = spec.resources.memory_mb.map(|mb| mb * 1024 * 1024);

        let mut endpoints_config = HashMap::new();
        if let Some(net) = &spec.network {
            let mut aliases = vec![spec.name.clone()];
            aliases.extend(spec.network_aliases.iter().cloned());
            endpoints_config.insert(
                net.clone(),
                EndpointSettings {
                    aliases: Some(aliases),
                    ..Default::default()
                },
            );
        }

        let mut labels = spec.labels.clone();
        labels.insert(ARX_OWNED_LABEL.to_string(), "true".to_string());

        let env: Vec<String> = spec.env.iter().map(|(k, v)| format!("{k}={v}")).collect();

        let host_config = HostConfig {
            mounts: Some(mounts),
            port_bindings: Some(port_bindings),
            restart_policy: Some(restart),
            nano_cpus,
            memory: memory_bytes,
            ..Default::default()
        };

        let config = Config {
            image: Some(spec.image.clone()),
            env: Some(env),
            exposed_ports: Some(
                exposed_ports
                    .into_keys()
                    .map(|k| (k, HashMap::new()))
                    .collect(),
            ),
            labels: Some(labels),
            host_config: Some(host_config),
            networking_config: if endpoints_config.is_empty() {
                None
            } else {
                Some(bollard::container::NetworkingConfig { endpoints_config })
            },
            ..Default::default()
        };

        let create_opts = CreateContainerOptions {
            name: spec.name.clone(),
            platform: None,
        };

        let created = self
            .docker
            .create_container(Some(create_opts), config)
            .await
            .map_err(|e| EngineError::CreateFailed(e.to_string()))?;

        self.docker
            .start_container::<&str>(&created.id, None)
            .await
            .map_err(|e| EngineError::StartFailed(e.to_string()))?;

        info!(container_id = %created.id, name = %spec.name, "container started");
        Ok(ContainerHandle(created.id))
    }

    async fn stop_and_remove(&self, handle: &ContainerHandle) -> Result<(), EngineError> {
        let stop_res = self
            .docker
            .stop_container(&handle.0, Some(StopContainerOptions { t: 10 }))
            .await;
        match stop_res {
            Ok(_) => {}
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => return Ok(()),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 304, ..
            }) => {}
            Err(e) => return Err(map_err(e)),
        }

        let remove_res = self
            .docker
            .remove_container(
                &handle.0,
                Some(RemoveContainerOptions {
                    force: true,
                    v: false,
                    ..Default::default()
                }),
            )
            .await;
        match remove_res {
            Ok(_) => Ok(()),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(()),
            Err(e) => Err(map_err(e)),
        }
    }

    async fn status(&self, handle: &ContainerHandle) -> Result<ContainerStatus, EngineError> {
        let info = self
            .docker
            .inspect_container(&handle.0, None)
            .await
            .map_err(map_err)?;
        let state = info.state.unwrap_or_default();
        let status = match state.status {
            Some(ContainerStateStatusEnum::CREATED) => ContainerStatus::Created,
            Some(ContainerStateStatusEnum::RUNNING) => ContainerStatus::Running,
            Some(ContainerStateStatusEnum::RESTARTING) => ContainerStatus::Restarting,
            Some(ContainerStateStatusEnum::PAUSED) => ContainerStatus::Paused,
            Some(ContainerStateStatusEnum::EXITED) => ContainerStatus::Exited {
                code: state.exit_code.unwrap_or(0),
            },
            Some(ContainerStateStatusEnum::DEAD) => ContainerStatus::Dead,
            Some(ContainerStateStatusEnum::REMOVING) => ContainerStatus::Removing,
            _ => ContainerStatus::Unknown,
        };
        Ok(status)
    }

    async fn internal_address(&self, handle: &ContainerHandle) -> Result<String, EngineError> {
        let info = self
            .docker
            .inspect_container(&handle.0, None)
            .await
            .map_err(map_err)?;

        if let Some(name) = info.name {
            let cleaned = name.trim_start_matches('/').to_string();
            if !cleaned.is_empty() {
                return Ok(cleaned);
            }
        }

        if let Some(net_settings) = info.network_settings {
            if let Some(networks) = net_settings.networks {
                for (_, endpoint) in networks {
                    if let Some(ip) = endpoint.ip_address {
                        if !ip.is_empty() {
                            return Ok(ip);
                        }
                    }
                }
            }
        }

        Err(EngineError::Other(
            "could not resolve internal address".to_string(),
        ))
    }

    async fn logs(&self, handle: &ContainerHandle, follow: bool) -> Result<LogStream, EngineError> {
        let opts = LogsOptions::<String> {
            follow,
            stdout: true,
            stderr: true,
            timestamps: true,
            ..Default::default()
        };
        let stream = self.docker.logs(&handle.0, Some(opts));
        let mapped = stream
            .map(|item| match item {
                Ok(LogOutput::StdOut { message }) | Ok(LogOutput::Console { message }) => {
                    Ok(LogLine {
                        stream: LogStreamKind::Stdout,
                        line: String::from_utf8_lossy(&message).into_owned(),
                        timestamp: chrono::Utc::now(),
                    })
                }
                Ok(LogOutput::StdErr { message }) => Ok(LogLine {
                    stream: LogStreamKind::Stderr,
                    line: String::from_utf8_lossy(&message).into_owned(),
                    timestamp: chrono::Utc::now(),
                }),
                Ok(LogOutput::StdIn { .. }) => Ok(LogLine {
                    stream: LogStreamKind::Stdout,
                    line: String::new(),
                    timestamp: chrono::Utc::now(),
                }),
                Err(e) => Err(EngineError::Io(e.to_string())),
            })
            .boxed();
        Ok(mapped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lifecycle_smoke() {
        if std::env::var("ARX_TEST_DOCKER").as_deref() != Ok("1") {
            return;
        }

        let engine = DockerEngine::connect_local().expect("docker connect");
        engine.ensure_network("arx-test-net").await.unwrap();

        let spec = ContainerSpec {
            image: "alpine:3".to_string(),
            name: format!("arx-test-{}", uuid::Uuid::new_v4()),
            env: vec![("FOO".into(), "bar".into())],
            ports: vec![],
            mounts: vec![],
            resources: ResourceLimits::default(),
            restart: RestartPolicy::No,
            network: Some("arx-test-net".to_string()),
            network_aliases: vec![],
            labels: HashMap::new(),
        };

        let h = engine.run(&spec).await.unwrap();
        let st = engine.status(&h).await.unwrap();
        assert!(matches!(
            st,
            ContainerStatus::Running | ContainerStatus::Exited { .. }
        ));
        engine.stop_and_remove(&h).await.unwrap();
    }
}
