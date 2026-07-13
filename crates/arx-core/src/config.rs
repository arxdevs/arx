use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,

    #[serde(default)]
    pub paths: PathsConfig,

    #[serde(default)]
    pub traefik: TraefikConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub listen: SocketAddr,

    pub public_url: Option<String>,

    pub public_ip: Option<std::net::IpAddr>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:7878".parse().unwrap(),
            public_url: None,
            public_ip: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub repos_dir: PathBuf,
    pub backups_dir: PathBuf,
    /// Where per-deployment build logs are stored (`<id>.log`). Defaulted so a
    /// pre-existing `[paths]` config that predates build logs still loads.
    #[serde(default = "default_build_logs_dir")]
    pub build_logs_dir: PathBuf,
    pub traefik_dir: PathBuf,

    pub master_key_path: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        let data = PathBuf::from("/var/lib/arx");
        Self {
            db_path: data.join("arx.db"),
            repos_dir: data.join("repos"),
            backups_dir: data.join("backups"),
            build_logs_dir: data.join("build-logs"),
            traefik_dir: data.join("traefik"),
            master_key_path: data.join("master.key"),
            data_dir: data,
        }
    }
}

fn default_build_logs_dir() -> PathBuf {
    PathBuf::from("/var/lib/arx/build-logs")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraefikConfig {
    pub admin_api_url: String,
    pub dynamic_config_path: PathBuf,
    pub static_config_path: PathBuf,
}

impl Default for TraefikConfig {
    fn default() -> Self {
        Self {
            admin_api_url: "http://traefik:8080".to_string(),
            dynamic_config_path: PathBuf::from("/var/lib/arx/traefik/dynamic.yml"),
            static_config_path: PathBuf::from("/var/lib/arx/traefik/static.yml"),
        }
    }
}

impl Config {
    pub fn load(path: impl AsRef<std::path::Path>) -> crate::Result<Self> {
        let bytes = std::fs::read_to_string(path.as_ref())?;
        let cfg: Config = toml::from_str(&bytes)?;
        Ok(cfg)
    }
}
