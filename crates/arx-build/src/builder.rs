use crate::stack::{self, CommandOverrides};
use crate::validate::BuildError;
use arx_core::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub struct BuildInput {
    pub source_dir: PathBuf,
    pub image_tag: String,
    pub dockerfile: Option<PathBuf>,
    pub root_directory: Option<PathBuf>,
    pub build_command: Option<String>,
    pub start_command: Option<String>,
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

    let context = match &input.root_directory {
        Some(rel) => contained(&source_root, &source_root.join(rel), "root_directory")?,
        None => source_root.clone(),
    };

    let explicit_dockerfile = match input.dockerfile.clone() {
        Some(p) => Some(contained(&source_root, &context.join(p), "dockerfile")?),
        None => {
            let f = context.join("Dockerfile");
            if f.exists() { Some(f) } else { None }
        }
    };

    if let Some(dockerfile) = explicit_dockerfile {
        docker_build_file(&context, &dockerfile, &input.image_tag).await?;
        return Ok(BuildOutput {
            image_ref: input.image_tag.clone(),
            used: BuilderKind::Dockerfile,
        });
    }

    let stack = stack::detect_stack(&context)
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
    tracing::info!(
        stack = stack_name,
        "rendered Dockerfile from stack template"
    );

    docker_build_stdin(&context, &dockerfile_text, &input.image_tag).await?;

    Ok(BuildOutput {
        image_ref: input.image_tag.clone(),
        used: BuilderKind::Stack { name: stack_name },
    })
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

async fn docker_build_file(context: &Path, dockerfile: &Path, tag: &str) -> Result<()> {
    let status = Command::new("docker")
        .args(["build", "-t", tag, "-f"])
        .arg(dockerfile)
        .arg(context)
        .stdin(Stdio::null())
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
async fn docker_build_stdin(context: &Path, dockerfile: &str, tag: &str) -> Result<()> {
    let mut child = Command::new("docker")
        .args(["build", "-t", tag, "-f", "-"])
        .arg(context)
        .stdin(Stdio::piped())
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
