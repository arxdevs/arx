use crate::render::{Route, render_dynamic_yaml};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tokio::time::sleep;

#[derive(Debug, Error)]
pub enum WriterError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("traefik api: {0}")]
    Api(String),

    #[error("timed out waiting for traefik to pick up route {0}")]
    Timeout(String),
}

#[derive(Clone)]
pub struct TraefikWriter {
    dynamic_path: PathBuf,
    admin_url: String,
    http: reqwest::Client,
}

impl TraefikWriter {
    pub fn new(dynamic_path: impl Into<PathBuf>, admin_url: impl Into<String>) -> Self {
        Self {
            dynamic_path: dynamic_path.into(),
            admin_url: admin_url.into(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("build http client"),
        }
    }

    pub fn write_routes(&self, routes: &[Route]) -> Result<(), WriterError> {
        let yaml = render_dynamic_yaml(routes);
        write_atomically(&self.dynamic_path, yaml.as_bytes())?;
        Ok(())
    }

    pub async fn confirm(
        &self,
        expected_ids: &[&str],
        timeout: Duration,
    ) -> Result<(), WriterError> {
        if expected_ids.is_empty() {
            return Ok(());
        }
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match self.fetch_routers().await {
                Ok(routers) => {
                    let enabled: std::collections::HashSet<&str> = routers
                        .iter()
                        .filter(|r| r.status.as_deref() == Some("enabled"))
                        .filter_map(|r| r.name.as_deref())
                        .collect();
                    if expected_ids.iter().all(|id| {
                        enabled
                            .iter()
                            .any(|name| name == id || name.starts_with(&format!("{id}@")))
                    }) {
                        return Ok(());
                    }
                }
                Err(e) => {
                    tracing::debug!(error = %e, "traefik api poll failed");
                }
            }
            if std::time::Instant::now() >= deadline {
                let missing: Vec<&str> = expected_ids.to_vec();
                return Err(WriterError::Timeout(missing.join(",")));
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    async fn fetch_routers(&self) -> Result<Vec<RouterInfo>, WriterError> {
        let url = format!("{}/api/http/routers", self.admin_url.trim_end_matches('/'));
        let res = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| WriterError::Api(e.to_string()))?;
        if !res.status().is_success() {
            return Err(WriterError::Api(format!("status {}", res.status())));
        }
        let body: Vec<RouterInfo> = res
            .json()
            .await
            .map_err(|e| WriterError::Api(e.to_string()))?;
        Ok(body)
    }
}

#[derive(Debug, Deserialize)]
struct RouterInfo {
    name: Option<String>,
    status: Option<String>,
}

fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    use std::io::Write;
    tmp.write_all(bytes)?;
    tmp.flush()?;
    tmp.persist(path).map_err(std::io::Error::other)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::BackendTarget;

    #[test]
    fn write_routes_is_atomic_and_overwrites() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("dynamic.yml");
        let writer = TraefikWriter::new(&path, "http://127.0.0.1:9");

        let r = vec![Route {
            id: "blog".into(),
            host: "blog.me.com".into(),
            backend: BackendTarget {
                host: "ctr".into(),
                port: 80,
            },
            tls: true,
        }];
        writer.write_routes(&r).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(first.contains("blog"));

        writer.write_routes(&[]).unwrap();
        let second = std::fs::read_to_string(&path).unwrap();
        assert!(!second.contains("blog"));
    }
}
