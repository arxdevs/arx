use arx_core::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

pub struct GitOpts {
    pub url: String,

    pub branch: String,

    pub token: Option<String>,
}

pub struct Cloner {
    pub workdir: PathBuf,
}

impl Cloner {
    pub fn new(workdir: PathBuf) -> Self {
        Self { workdir }
    }

    pub async fn checkout(&self, key: &str, opts: &GitOpts) -> Result<(PathBuf, String)> {
        std::fs::create_dir_all(&self.workdir).map_err(Error::from)?;
        let path = self.workdir.join(key);

        let url = opts.url_with_token();

        if path.join(".git").exists() {
            run(&path, &["git", "remote", "set-url", "origin", &url]).await?;
            run(
                &path,
                &["git", "fetch", "--depth=1", "origin", &opts.branch],
            )
            .await?;
            run(&path, &["git", "checkout", &opts.branch]).await?;
            run(
                &path,
                &["git", "reset", "--hard", &format!("origin/{}", opts.branch)],
            )
            .await?;
        } else {
            std::fs::create_dir_all(&path).map_err(Error::from)?;
            let status = Command::new("git")
                .args(["clone", "--depth=1", "--branch", &opts.branch, &url])
                .arg(&path)
                .current_dir(Path::new("."))
                .stdin(Stdio::null())
                .status()
                .await
                .map_err(|e| Error::Internal(format!("spawn git: {e}")))?;
            if !status.success() {
                return Err(Error::Internal(format!(
                    "git clone failed: exit {}",
                    status.code().unwrap_or(-1)
                )));
            }
        }

        let head = run_capture(&path, &["git", "rev-parse", "HEAD"]).await?;
        Ok((path, head.trim().to_string()))
    }
}

impl GitOpts {
    fn url_with_token(&self) -> String {
        match &self.token {
            Some(tok) => {
                if let Some(rest) = self.url.strip_prefix("https://") {
                    format!("https://x-access-token:{tok}@{rest}")
                } else {
                    self.url.clone()
                }
            }
            None => self.url.clone(),
        }
    }
}

async fn run(cwd: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new(args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .status()
        .await
        .map_err(|e| Error::Internal(format!("spawn {}: {e}", args[0])))?;
    if !status.success() {
        return Err(Error::Internal(format!(
            "{} failed: exit {}",
            args.join(" "),
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

async fn run_capture(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new(args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("spawn {}: {e}", args[0])))?;
    if !out.status.success() {
        return Err(Error::Internal(format!(
            "{} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
