use std::path::PathBuf;
use std::time::Duration;

const INTERVAL_SECS: u64 = 24 * 60 * 60;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Cache {
    last_check: u64,
    latest: String,
}

fn cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".arx/update-check.json"))
}

/// Best-effort "a newer release exists" notice, printed to stderr after a
/// command completes. Throttled to once per 24h via `~/.arx/update-check.json`;
/// any network or IO failure is swallowed so this never disrupts the CLI.
pub async fn notify() {
    if std::env::var_os("ARX_NO_UPDATE_CHECK").is_some() {
        return;
    }
    let Some(path) = cache_path() else {
        return;
    };
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => return,
    };

    let cached: Cache = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();

    let latest = if !cached.latest.is_empty()
        && now.saturating_sub(cached.last_check) < INTERVAL_SECS
    {
        cached.latest
    } else {
        // Refresh with a short timeout; silently give up on failure.
        let Ok(tag) = crate::update_cmd::latest_version(Duration::from_millis(1500)).await else {
            return;
        };
        let updated = Cache {
            last_check: now,
            latest: tag.clone(),
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(buf) = serde_json::to_vec(&updated) {
            let _ = std::fs::write(&path, buf);
        }
        tag
    };

    let current = env!("CARGO_PKG_VERSION");
    let newer = matches!(
        (
            semver::Version::parse(latest.trim_start_matches('v')),
            semver::Version::parse(current),
        ),
        (Ok(l), Ok(c)) if l > c
    );
    if newer {
        eprintln!("\nA new version of arx is available: {latest} (current v{current})");
        eprintln!("Run `arx update` to upgrade.");
    }
}
