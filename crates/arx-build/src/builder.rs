use crate::monorepo::{self, MonorepoLayout, WorkspaceContext};
use crate::stack::{self, CommandOverrides, StackBuilder};
use crate::validate::BuildError;
use arx_core::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use sha2::{Digest, Sha256};

pub struct BuildInput {
    pub source_dir: PathBuf,
    pub image_tag: String,
    pub dockerfile: Option<PathBuf>,
    pub root_directory: Option<PathBuf>,
    pub build_command: Option<String>,
    pub start_command: Option<String>,
    /// Resolved service variables, available at build time (Railway parity).
    /// Injected as a single BuildKit secret (`arx_env`) — never as `--build-arg`
    /// — so values never leak into image history/layers.
    pub build_env: Vec<(String, String)>,
}

pub struct BuildOutput {
    pub image_ref: String,
    pub used: BuilderKind,
}

#[derive(Debug, Clone)]
pub enum BuilderKind {
    Dockerfile,
    Stack { name: &'static str },
}

pub struct Builder;

pub async fn build(input: &BuildInput) -> Result<BuildOutput> {
    let source_root = input
        .source_dir
        .canonicalize()
        .map_err(|e| Error::Internal(format!("canonicalize source_dir: {e}")))?;

    let package_dir = match &input.root_directory {
        Some(rel) => contained(&source_root, &source_root.join(rel), "root_directory")?,
        None => source_root.clone(),
    };

    let monorepo = monorepo::detect(&source_root, input.root_directory.as_deref());

    let context = match &monorepo {
        Some(m) => contained(&source_root, &m.root, "monorepo_root")?,
        None => package_dir.clone(),
    };

    let prepared = prepare_build_env(&input.build_env);

    // The explicit Dockerfile path is resolved against the package directory —
    // a Dockerfile lives with its app, regardless of monorepo layout.
    let explicit_dockerfile = match input.dockerfile.clone() {
        Some(p) => Some(contained(&source_root, &package_dir.join(p), "dockerfile")?),
        None => {
            let f = package_dir.join("Dockerfile");
            if f.exists() { Some(f) } else { None }
        }
    };

    if let Some(dockerfile) = explicit_dockerfile {
        docker_build_file(&context, &dockerfile, &input.image_tag, &prepared).await?;
        return Ok(BuildOutput {
            image_ref: input.image_tag.clone(),
            used: BuilderKind::Dockerfile,
        });
    }

    let stack = build_stack(&package_dir, monorepo.as_ref())
        .ok_or_else(|| Error::InvalidInput(BuildError::NoStack.to_string()))?;

    let overrides = CommandOverrides {
        build_command: input.build_command.as_deref(),
        start_command: input.start_command.as_deref(),
    };
    let dockerfile_text = stack.render_dockerfile(&overrides).map_err(|e| match e {
        BuildError::InvalidInput { .. }
        | BuildError::NoStack
        | BuildError::StackRequiresField { .. } => Error::InvalidInput(e.to_string()),
        other => Error::Internal(other.to_string()),
    })?;
    let stack_name = stack.name();
    if let Some(m) = &monorepo {
        tracing::info!(
            stack = stack_name,
            monorepo_root = ?m.root,
            workspace_kind = ?m.kind,
            package = ?m.package_rel_path,
            "rendered Dockerfile (workspace-aware)"
        );
    } else {
        tracing::info!(
            stack = stack_name,
            "rendered Dockerfile from stack template"
        );
    }

    docker_build_stdin(&context, &dockerfile_text, &input.image_tag, &prepared).await?;

    Ok(BuildOutput {
        image_ref: input.image_tag.clone(),
        used: BuilderKind::Stack { name: stack_name },
    })
}

/// When a monorepo is detected, prefer Node and inject the workspace context.
/// Other stacks (Go/Python/Java) fall through to the regular detector — they
/// don't currently understand workspaces.
fn build_stack(
    package_dir: &Path,
    monorepo: Option<&MonorepoLayout>,
) -> Option<Box<dyn StackBuilder>> {
    if let Some(m) = monorepo {
        if let Some(node) = crate::stacks::Node::detect_concrete(package_dir) {
            let node = (*node).rebind_pm_from(&m.root);
            let ws = WorkspaceContext {
                kind: m.kind,
                package_rel_path: m
                    .package_rel_path
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/"),
                package_name: monorepo::read_package_name(package_dir),
                workspace_manifests: monorepo::workspace_manifest_paths(&m.root),
            };
            return Some(node.with_workspace(ws));
        }
    }
    stack::detect_stack(package_dir)
}

fn contained(root: &Path, candidate: &Path, label: &str) -> Result<PathBuf> {
    let resolved = candidate
        .canonicalize()
        .map_err(|e| Error::InvalidInput(format!("{label} {candidate:?}: {e}")))?;
    if !resolved.starts_with(root) {
        return Err(Error::InvalidInput(format!(
            "{label} escapes source tree: {resolved:?}"
        )));
    }
    Ok(resolved)
}

/// Service variables prepared for build-time injection. The payload is a set of
/// shell-safe `export NAME='value'` lines delivered as a single BuildKit secret;
/// the hash is a non-secret digest passed as a build-arg to bust the layer cache
/// when any value changes.
struct PreparedEnv {
    secret_payload: Option<String>,
    hash: Option<String>,
}

fn prepare_build_env(vars: &[(String, String)]) -> PreparedEnv {
    let mut payload = String::new();
    for (k, v) in vars {
        if crate::validate::validate_env_name(k).is_err() {
            tracing::warn!(key = %k, "skipping build env var: invalid name");
            continue;
        }
        // single-quote escaping makes the value inert when sourced — `;`, `$(...)`
        // and other shell metacharacters cannot execute. Rejects newline/NUL.
        let escaped = match crate::validate::shell_single_quote(v, "build_env_value") {
            Ok(s) => s,
            Err(_) => {
                tracing::warn!(key = %k, "skipping build env var: value rejected");
                continue;
            }
        };
        payload.push_str("export ");
        payload.push_str(k);
        payload.push_str("='");
        payload.push_str(&escaped);
        payload.push_str("'\n");
    }
    if payload.is_empty() {
        return PreparedEnv {
            secret_payload: None,
            hash: None,
        };
    }
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    PreparedEnv {
        secret_payload: Some(payload),
        hash: Some(hash),
    }
}

/// Enable BuildKit (for `--secret`/`--mount=type=secret`) and attach the service
/// env secret + cache-busting build-arg. NOTE: this uses Docker's built-in
/// BuildKit frontend — it is NOT the in-container build agent (railpack/nixpacks)
/// that AGENTS.md forbids reintroducing.
fn apply_build_env(cmd: &mut Command, env: &PreparedEnv) {
    cmd.env("DOCKER_BUILDKIT", "1");
    if let Some(h) = &env.hash {
        cmd.arg("--build-arg").arg(format!("ARX_ENV_HASH={h}"));
    }
    if let Some(p) = &env.secret_payload {
        cmd.env("ARX_ENV_SECRET", p);
        cmd.arg("--secret").arg("id=arx_env,env=ARX_ENV_SECRET");
    }
}

async fn docker_build_file(
    context: &Path,
    dockerfile: &Path,
    tag: &str,
    env: &PreparedEnv,
) -> Result<()> {
    let mut cmd = Command::new("docker");
    cmd.arg("build").arg("-t").arg(tag);
    apply_build_env(&mut cmd, env);
    cmd.arg("-f")
        .arg(dockerfile)
        .arg(context)
        .stdin(Stdio::null());
    let status = cmd
        .status()
        .await
        .map_err(|e| Error::Internal(format!("docker build spawn: {e}")))?;
    if !status.success() {
        return Err(Error::Internal(format!(
            "docker build failed (exit {})",
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

/// Avoids writing a Dockerfile into the user's repo by piping through `-f -`.
async fn docker_build_stdin(
    context: &Path,
    dockerfile: &str,
    tag: &str,
    env: &PreparedEnv,
) -> Result<()> {
    let mut cmd = Command::new("docker");
    cmd.arg("build").arg("-t").arg(tag);
    apply_build_env(&mut cmd, env);
    cmd.arg("-f").arg("-").arg(context).stdin(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Internal(format!("docker build spawn: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(dockerfile.as_bytes())
            .await
            .map_err(|e| Error::Internal(format!("write Dockerfile to docker build stdin: {e}")))?;
        stdin
            .shutdown()
            .await
            .map_err(|e| Error::Internal(format!("close docker build stdin: {e}")))?;
    }

    let status = child
        .wait()
        .await
        .map_err(|e| Error::Internal(format!("docker build wait: {e}")))?;
    if !status.success() {
        return Err(Error::Internal(format!(
            "docker build failed (exit {})",
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stack::CommandOverrides;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn prepare_build_env_escapes_skips_and_hashes() {
        let p = prepare_build_env(&[
            ("A".into(), "b; rm -rf /".into()),
            ("Q".into(), "a'b".into()),
            ("BAD-NAME".into(), "x".into()),
        ]);
        let payload = p.secret_payload.expect("payload");
        // shell metacharacters are inert inside single quotes
        assert!(payload.contains("export A='b; rm -rf /'"));
        // single quote in value is escaped as '\''
        assert!(payload.contains(r"export Q='a'\''b'"));
        // invalid identifier is skipped
        assert!(!payload.contains("BAD-NAME"));
        assert!(p.hash.is_some());
    }

    #[test]
    fn prepare_build_env_empty_is_none() {
        let p = prepare_build_env(&[]);
        assert!(p.secret_payload.is_none());
        assert!(p.hash.is_none());
    }

    #[test]
    fn workspace_aware_node_dockerfile_for_pnpm_monorepo() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Monorepo root
        fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - 'apps/*'\n",
        )
        .unwrap();
        fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: 9").unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"name":"root","private":true}"#,
        )
        .unwrap();
        // Package
        let pkg = root.join("apps/web");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(
            pkg.join("package.json"),
            r#"{"name":"web","scripts":{"start":"next start"}}"#,
        )
        .unwrap();

        let layout = monorepo::detect(root, Some(Path::new("apps/web"))).unwrap();
        let stack = build_stack(&pkg, Some(&layout)).expect("stack");
        let text = stack
            .render_dockerfile(&CommandOverrides::default())
            .unwrap();

        assert!(text.contains("pnpm --filter ./apps/web run start"));
        assert!(text.contains("pnpm --filter ./apps/web run build"));
        assert!(text.contains("COPY pnpm-lock.yaml*"));
        assert!(text.contains("COPY pnpm-workspace.yaml*"));
    }

    #[test]
    fn single_app_skips_workspace_logic() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"app","scripts":{"start":"node ."}}"#,
        )
        .unwrap();
        fs::write(root.join("package-lock.json"), "{}").unwrap();

        // No monorepo markers
        let layout = monorepo::detect(root, None);
        assert!(layout.is_none());

        let stack = build_stack(root, layout.as_ref()).expect("stack");
        let text = stack
            .render_dockerfile(&CommandOverrides::default())
            .unwrap();

        assert!(text.contains("npm ci"));
        assert!(!text.contains("--filter"));
        assert!(!text.contains("-w "));
    }

    #[test]
    fn root_lockfile_overrides_package_lockfile_in_monorepo() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Root uses pnpm
        fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - 'apps/*'\n",
        )
        .unwrap();
        fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: 9").unwrap();
        // Package accidentally has yarn.lock — should be ignored
        let pkg = root.join("apps/web");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("package.json"), r#"{"name":"web"}"#).unwrap();
        fs::write(pkg.join("yarn.lock"), "").unwrap();

        let layout = monorepo::detect(root, Some(Path::new("apps/web"))).unwrap();
        let stack = build_stack(&pkg, Some(&layout)).expect("stack");
        let text = stack
            .render_dockerfile(&CommandOverrides::default())
            .unwrap();

        assert!(text.contains("pnpm install --frozen-lockfile"));
        assert!(!text.contains("yarn install"));
    }
}
