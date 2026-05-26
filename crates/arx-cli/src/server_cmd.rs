use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const COMPOSE_URL: &str = "https://raw.githubusercontent.com/jbj338033/arx/main/compose.yml";

pub fn compose_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".arx/compose.yml"))
}

pub async fn install(quiet: bool) -> Result<()> {
    let path = compose_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if !path.exists() {
        if !quiet {
            eprintln!("fetching compose.yml → {}", path.display());
        }
        download_compose(&path).await?;
    } else if !quiet {
        eprintln!("using existing compose.yml at {}", path.display());
    }

    compose_cmd(&["up", "-d"], &path).await?;
    if !quiet {
        eprintln!("waiting for daemon healthy...");
    }
    wait_healthy("http://127.0.0.1:7878").await?;
    if !quiet {
        eprintln!("✓ daemon ready");
    }
    Ok(())
}

pub async fn upgrade(quiet: bool) -> Result<()> {
    let path = compose_path()?;
    if !path.exists() {
        bail!(
            "no compose.yml at {} — run `arx server install` first",
            path.display()
        );
    }

    let bak = path.with_extension("yml.bak");
    std::fs::copy(&path, &bak).context("backup compose.yml")?;
    if !quiet {
        eprintln!("backed up {} → {}", path.display(), bak.display());
    }

    download_compose(&path).await?;

    compose_cmd(&["pull"], &path).await?;
    compose_cmd(&["up", "-d"], &path).await?;
    wait_healthy("http://127.0.0.1:7878").await?;
    if !quiet {
        eprintln!("✓ upgrade complete");
    }
    Ok(())
}

pub async fn status() -> Result<()> {
    let path = compose_path()?;
    if !path.exists() {
        bail!(
            "no compose.yml at {} (daemon not installed)",
            path.display()
        );
    }
    compose_cmd(&["ps"], &path).await?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()?;
    match http.get("http://127.0.0.1:7878/health").send().await {
        Ok(r) if r.status().is_success() => println!("/health: ok"),
        Ok(r) => println!("/health: status {}", r.status()),
        Err(e) => println!("/health: error {e}"),
    }
    Ok(())
}

async fn download_compose(target: &PathBuf) -> Result<()> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let body = http
        .get(COMPOSE_URL)
        .send()
        .await
        .with_context(|| format!("fetch {COMPOSE_URL}"))?
        .error_for_status()?
        .text()
        .await?;
    std::fs::write(target, body).with_context(|| format!("write {}", target.display()))?;
    Ok(())
}

async fn compose_cmd(args: &[&str], path: &PathBuf) -> Result<()> {
    let status = Command::new("docker")
        .arg("compose")
        .arg("--project-name")
        .arg("arx")
        .arg("--file")
        .arg(path)
        .args(args)
        .stdin(Stdio::null())
        .status()
        .await
        .context("spawn docker compose")?;
    if !status.success() {
        bail!(
            "docker compose {} failed: exit {}",
            args.join(" "),
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

async fn wait_healthy(base: &str) -> Result<()> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        if let Ok(r) = http.get(format!("{base}/health")).send().await {
            if r.status().is_success() {
                return Ok(());
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    bail!("daemon did not become healthy in 60s")
}
