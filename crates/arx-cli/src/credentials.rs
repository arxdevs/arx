use crate::error::CliError;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct CredentialEntry {
    pub url: String,
    pub token: String,
}

pub(crate) fn credentials_path(override_path: Option<&PathBuf>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        return Ok(p.clone());
    }
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".arx/credentials.json"))
}

pub(crate) fn load_credentials(path: &PathBuf) -> Result<Vec<CredentialEntry>> {
    if path.exists() {
        let s = std::fs::read_to_string(path).context("read credentials")?;
        if s.trim().is_empty() {
            return Ok(Vec::new());
        }
        let v: Vec<CredentialEntry> = serde_json::from_str(&s).context("parse credentials.json")?;
        return Ok(v);
    }

    let legacy = path.with_file_name("credentials");
    if legacy.exists() {
        let s = std::fs::read_to_string(&legacy).context("read legacy credentials")?;
        #[derive(Deserialize)]
        struct Old {
            server: Option<String>,
            token: Option<String>,
        }
        let old: Old = toml::from_str(&s).context("parse legacy credentials TOML")?;
        match (old.server, old.token) {
            (Some(server), Some(token)) => Ok(vec![CredentialEntry { url: server, token }]),
            _ => Ok(Vec::new()),
        }
    } else {
        Ok(Vec::new())
    }
}

pub(crate) fn save_credentials(path: &PathBuf, entries: &[CredentialEntry]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(entries)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        std::io::Write::write_all(&mut f, s.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, s)?;
    }
    Ok(())
}

pub(crate) fn resolve_target(
    entries: &[CredentialEntry],
    cli_server: &str,
    cli_server_explicit: bool,
) -> Result<(String, Option<String>)> {
    if cli_server_explicit {
        if let Some(e) = entries.iter().find(|e| e.url == cli_server) {
            return Ok((e.url.clone(), Some(e.token.clone())));
        }
        return Ok((cli_server.to_string(), None));
    }
    match entries.len() {
        0 => Ok((cli_server.to_string(), None)),
        1 => Ok((entries[0].url.clone(), Some(entries[0].token.clone()))),
        _ => Err(CliError::Usage(
            "multiple servers in credentials.json; specify --server URL".into(),
        )
        .into()),
    }
}

pub(crate) fn upsert_credential(entries: &mut Vec<CredentialEntry>, url: &str, token: &str) {
    if let Some(e) = entries.iter_mut().find(|e| e.url == url) {
        e.token = token.to_string();
    } else {
        entries.push(CredentialEntry {
            url: url.to_string(),
            token: token.to_string(),
        });
    }
}

pub(crate) fn remove_credential(entries: &mut Vec<CredentialEntry>, url: &str) {
    entries.retain(|e| e.url != url);
}

pub(crate) fn upsert_and_save(path: &PathBuf, url: &str, token: &str) -> Result<()> {
    let mut entries = load_credentials(path)?;
    upsert_credential(&mut entries, url, token);
    save_credentials(path, &entries)
}
