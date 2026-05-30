use anyhow::{Context, Result, bail};
use std::path::Path;
use std::time::Duration;

const REPO: &str = "arxdevs/arx";

/// Map the host platform to the release asset naming used by `install.sh`
/// (`arx-{os}-{arch}.tar.gz`).
fn target() -> Result<(&'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        other => bail!("unsupported OS for self-update: {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => bail!("unsupported arch for self-update: {other}"),
    };
    Ok((os, arch))
}

/// Resolve the latest release tag (e.g. `v0.1.9`) by following the
/// `github.com/<repo>/releases/latest` redirect — mirrors `install.sh`,
/// avoiding any dependency on the GitHub API or `jq`.
pub async fn latest_version(timeout: Duration) -> Result<String> {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()?;
    let url = format!("https://github.com/{REPO}/releases/latest");
    let resp = http
        .get(&url)
        .send()
        .await
        .context("reach github.com releases/latest")?;
    let location = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .context("no redirect from releases/latest")?;
    // location ends with .../releases/tag/<version>
    let tag = location
        .rsplit("/tag/")
        .next()
        .filter(|t| !t.is_empty() && *t != location)
        .context("could not parse latest version tag")?;
    Ok(tag.to_string())
}

/// `arx update` — self-update the CLI, then upgrade the local daemon when one
/// is installed (unless `--cli-only`).
pub async fn run(quiet: bool, cli_only: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let pinned = std::env::var("ARX_VERSION").ok().filter(|s| !s.is_empty());

    let tag = match &pinned {
        Some(v) => v.clone(),
        None => latest_version(Duration::from_secs(15)).await?,
    };
    let latest = tag.trim_start_matches('v');

    let newer = match (
        semver::Version::parse(latest),
        semver::Version::parse(current),
    ) {
        (Ok(l), Ok(c)) => l > c,
        // Non-semver tag (or pin): fall back to a plain inequality.
        _ => latest != current,
    };

    if pinned.is_none() && !newer {
        if !quiet {
            eprintln!("arx is already up to date (v{current})");
        }
    } else {
        update_binary(&tag, current, quiet).await?;
    }

    if !cli_only {
        if crate::server_cmd::compose_path()?.exists() {
            if !quiet {
                eprintln!("updating local daemon...");
            }
            crate::server_cmd::upgrade(quiet).await?;
        } else if !quiet {
            eprintln!("no local daemon (~/.arx/compose.yml absent) — updated CLI only");
        }
    }
    Ok(())
}

async fn update_binary(tag: &str, current: &str, quiet: bool) -> Result<()> {
    let (os, arch) = target()?;
    let asset = format!("arx-{os}-{arch}.tar.gz");
    let base = format!("https://github.com/{REPO}/releases/download/{tag}/{asset}");

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;

    if !quiet {
        eprintln!("downloading {asset} ({tag})...");
    }
    let archive = http
        .get(&base)
        .send()
        .await
        .with_context(|| format!("download {base}"))?
        .error_for_status()
        .with_context(|| format!("download {asset} (does the release/arch exist?)"))?
        .bytes()
        .await?;

    let sums = http
        .get(format!("{base}.sha256"))
        .send()
        .await
        .with_context(|| format!("download {asset}.sha256"))?
        .error_for_status()?
        .text()
        .await?;
    // `shasum -a 256` output: "<hex>  <filename>".
    let expected = sums
        .split_whitespace()
        .next()
        .context("empty sha256 file")?
        .to_lowercase();

    use sha2::{Digest, Sha256};
    let actual = format!("{:x}", Sha256::digest(&archive));
    if actual != expected {
        bail!("checksum mismatch for {asset}: expected {expected}, got {actual}");
    }

    let tmp_bin = std::env::temp_dir().join(format!("arx-update-{}", std::process::id()));
    extract_arx(archive.as_ref(), &tmp_bin)?;

    self_replace::self_replace(&tmp_bin).context("replace the running arx binary")?;
    let _ = std::fs::remove_file(&tmp_bin);

    if !quiet {
        eprintln!("✓ updated arx v{current} → {tag}");
    }
    Ok(())
}

/// Extract the `arx` entry from a `.tar.gz` archive into `dest` (0755).
fn extract_arx(archive: &[u8], dest: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::io::Read;

    let mut tar = tar::Archive::new(GzDecoder::new(archive));
    for entry in tar.entries().context("read release archive")? {
        let mut entry = entry?;
        let is_arx = entry
            .path()?
            .file_name()
            .map(|n| n == "arx")
            .unwrap_or(false);
        if is_arx {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            std::fs::write(dest, &buf).with_context(|| format!("write {}", dest.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755))?;
            }
            return Ok(());
        }
    }
    bail!("`arx` binary not found in release archive")
}
