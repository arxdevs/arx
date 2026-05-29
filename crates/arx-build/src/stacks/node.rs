use crate::monorepo::{WorkspaceContext, WorkspaceKind};
use crate::stack::{CommandOverrides, StackBuilder, StackDetector};
use crate::validate::{self, BuildError};
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pm {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl Pm {
    fn install_cmd(self, has_lock: bool) -> &'static str {
        match (self, has_lock) {
            (Pm::Npm, true) => "npm ci",
            (Pm::Npm, false) => "npm install",
            (Pm::Pnpm, true) => "pnpm install --frozen-lockfile",
            (Pm::Pnpm, false) => "pnpm install",
            (Pm::Yarn, true) => "yarn install --frozen-lockfile",
            (Pm::Yarn, false) => "yarn install",
            (Pm::Bun, true) => "bun install --frozen-lockfile",
            (Pm::Bun, false) => "bun install",
        }
    }
    fn run_cmd(self) -> &'static str {
        match self {
            Pm::Npm => "npm start",
            Pm::Pnpm => "pnpm start",
            Pm::Yarn => "yarn start",
            Pm::Bun => "bun start",
        }
    }

    fn corepack_line(self) -> &'static str {
        match self {
            Pm::Npm | Pm::Bun => "",
            Pm::Pnpm => "RUN corepack enable && corepack prepare pnpm@latest --activate\n",
            Pm::Yarn => "RUN corepack enable && corepack prepare yarn@stable --activate\n",
        }
    }

    fn base_image_extra(self) -> &'static str {
        match self {
            Pm::Bun => {
                "RUN apt-get update && apt-get install -y --no-install-recommends curl unzip ca-certificates \
                 && rm -rf /var/lib/apt/lists/* \
                 && curl -fsSL https://bun.sh/install | bash \
                 && ln -s /root/.bun/bin/bun /usr/local/bin/bun\n"
            }
            _ => "",
        }
    }
}

#[derive(Debug)]
pub struct Node {
    node_major: u8,
    pm: Pm,
    has_lock: bool,
    has_start_script: bool,
    workspace: Option<WorkspaceContext>,
}

impl Node {
    /// Promote a single-app `Node` into a monorepo-aware build.
    ///
    /// Called by `builder.rs` after `monorepo::detect` finds a workspace marker
    /// in an ancestor of the service's `root_directory`.
    pub fn with_workspace(mut self, ws: WorkspaceContext) -> Box<dyn StackBuilder> {
        self.workspace = Some(ws);
        Box::new(self)
    }

    /// Re-detect the package manager from a different directory (the monorepo
    /// root, not the per-package `root_directory`). The root lockfile is the
    /// authoritative one for workspace installs.
    pub fn rebind_pm_from(mut self, dir: &Path) -> Self {
        let (pm, has_lock) = detect_pm(dir);
        self.pm = pm;
        self.has_lock = has_lock;
        self
    }
}

impl Node {
    pub fn detect_concrete(source_dir: &Path) -> Option<Box<Node>> {
        let pkg_path = source_dir.join("package.json");
        if !pkg_path.exists() {
            return None;
        }
        let raw = read_capped(&pkg_path, 256 * 1024).ok()?;
        let pkg: Value = serde_json::from_str(&raw).ok()?;
        let engines_node = pkg
            .get("engines")
            .and_then(|v| v.get("node"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let has_start_script = pkg.get("scripts").and_then(|v| v.get("start")).is_some();

        let nvmrc = source_dir.join(".nvmrc");
        let node_version_file = source_dir.join(".node-version");
        let from_file = [&nvmrc, &node_version_file]
            .into_iter()
            .find_map(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| validate::parse_node_major(s.trim()).ok());

        let from_engines = engines_node
            .as_deref()
            .and_then(validate::parse_node_major_from_engines);

        let node_major = from_file.or(from_engines).unwrap_or(22);

        let (pm, has_lock) = detect_pm(source_dir);

        Some(Box::new(Node {
            node_major,
            pm,
            has_lock,
            has_start_script,
            workspace: None,
        }))
    }
}

impl StackDetector for Node {
    fn detect(source_dir: &Path) -> Option<Box<dyn StackBuilder>> {
        Node::detect_concrete(source_dir).map(|b| b as Box<dyn StackBuilder>)
    }
}

impl StackBuilder for Node {
    fn name(&self) -> &'static str {
        "node"
    }

    fn render_dockerfile(&self, ov: &CommandOverrides<'_>) -> Result<String, BuildError> {
        match &self.workspace {
            None => self.render_single_app(ov),
            Some(ws) => self.render_workspace(ov, ws),
        }
    }
}

impl Node {
    fn render_single_app(&self, ov: &CommandOverrides<'_>) -> Result<String, BuildError> {
        let default_build = self.pm.install_cmd(self.has_lock);
        let default_start = if self.has_start_script {
            self.pm.run_cmd().to_string()
        } else {
            "node index.js".to_string()
        };

        let build_raw = ov.build_command.unwrap_or(default_build);
        let start_raw = ov.start_command.unwrap_or(default_start.as_str());

        let build_quoted = validate::shell_single_quote(build_raw, "build_command")?;
        let build_run = crate::stack::build_run_with_env(&build_quoted);
        let start_json = validate::cmd_to_json_token(start_raw, "start_command")?;
        let node_major = self.node_major;

        let pm_install = self.pm.corepack_line();
        let bun_install = self.pm.base_image_extra();

        Ok(format!(
            "# syntax=docker/dockerfile:1.7\n\
             FROM node:{node_major}-bookworm-slim\n\
             WORKDIR /app\n\
             {bun_install}\
             {pm_install}\
             COPY . .\n\
             {build_run}\n\
             ENV PORT=8080\n\
             EXPOSE 8080\n\
             CMD [\"sh\",\"-c\",{start_json}]\n"
        ))
    }

    fn render_workspace(
        &self,
        ov: &CommandOverrides<'_>,
        ws: &WorkspaceContext,
    ) -> Result<String, BuildError> {
        let filter = workspace_filter_token(self.pm, ws)?;
        let install_cmd = self.pm.install_cmd(self.has_lock);
        let default_build = format!("{install_cmd} && {filter} run build");
        let default_start = format!("{filter} run start");

        let build_raw = ov.build_command.unwrap_or(default_build.as_str());
        let start_raw = ov.start_command.unwrap_or(default_start.as_str());

        let build_quoted = validate::shell_single_quote(build_raw, "build_command")?;
        let build_run = crate::stack::build_run_with_env(&build_quoted);
        let start_json = validate::cmd_to_json_token(start_raw, "start_command")?;
        let install_quoted = validate::shell_single_quote(install_cmd, "build_command")?;
        let node_major = self.node_major;
        let pm_install = self.pm.corepack_line();
        let bun_install = self.pm.base_image_extra();

        // Layer caching: copy lockfile + workspace metadata first to warm the
        // package-manager cache, then copy the full tree and install again.
        // The second install is almost entirely a cache hit.
        let lockfile_line = match self.pm {
            Pm::Pnpm => "COPY pnpm-lock.yaml* ./\n",
            Pm::Npm => "COPY package-lock.json* ./\n",
            Pm::Yarn => "COPY yarn.lock* ./\n",
            Pm::Bun => "COPY bun.lockb* ./\n",
        };
        let workspace_meta_line = match ws.kind {
            WorkspaceKind::Pnpm => "COPY pnpm-workspace.yaml* ./\nCOPY turbo.json* ./\n",
            WorkspaceKind::Turbo => "COPY turbo.json* ./\nCOPY pnpm-workspace.yaml* ./\n",
            WorkspaceKind::NpmYarnBun => "COPY turbo.json* ./\n",
        };

        Ok(format!(
            "# syntax=docker/dockerfile:1.7\n\
             FROM node:{node_major}-bookworm-slim\n\
             WORKDIR /app\n\
             {bun_install}\
             {pm_install}\
             COPY package.json ./\n\
             {lockfile_line}\
             {workspace_meta_line}\
             RUN sh -c '{install_quoted}' || true\n\
             COPY . .\n\
             {build_run}\n\
             ENV PORT=8080\n\
             EXPOSE 8080\n\
             CMD [\"sh\",\"-c\",{start_json}]\n"
        ))
    }
}

fn workspace_filter_token(pm: Pm, ws: &WorkspaceContext) -> Result<String, BuildError> {
    let path = ws.package_rel_path.as_str();
    if path.is_empty() || path.contains("..") || path.starts_with('/') {
        return Err(BuildError::InvalidInput {
            field: "workspace_package_path",
            reason: "invalid package path".into(),
        });
    }
    match pm {
        Pm::Pnpm => Ok(format!("pnpm --filter ./{path}")),
        Pm::Bun => Ok(format!("bun --filter ./{path}")),
        Pm::Npm => Ok(format!("npm -w {path}")),
        Pm::Yarn => {
            let name = ws
                .package_name
                .as_deref()
                .ok_or(BuildError::StackRequiresField {
                    stack: "node-workspace-yarn",
                    field: "package.json#name",
                })?;
            if name.contains('\'') || name.contains('\n') || name.is_empty() {
                return Err(BuildError::InvalidInput {
                    field: "package.json#name",
                    reason: "invalid characters".into(),
                });
            }
            Ok(format!("yarn workspace {name}"))
        }
    }
}

fn detect_pm(source_dir: &Path) -> (Pm, bool) {
    if source_dir.join("pnpm-lock.yaml").exists() {
        return (Pm::Pnpm, true);
    }
    if source_dir.join("bun.lockb").exists() || source_dir.join("bun.lock").exists() {
        return (Pm::Bun, true);
    }
    if source_dir.join("yarn.lock").exists() {
        return (Pm::Yarn, true);
    }
    let has_lock = source_dir.join("package-lock.json").exists();
    (Pm::Npm, has_lock)
}

fn read_capped(p: &Path, cap: u64) -> std::io::Result<String> {
    let meta = std::fs::metadata(p)?;
    if meta.len() > cap {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "manifest too large",
        ));
    }
    std::fs::read_to_string(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(kind: WorkspaceKind, path: &str, name: Option<&str>) -> WorkspaceContext {
        WorkspaceContext {
            kind,
            package_rel_path: path.to_string(),
            package_name: name.map(String::from),
        }
    }

    fn render(node: Node) -> String {
        node.render_dockerfile(&CommandOverrides::default())
            .unwrap()
    }

    #[test]
    fn single_app_pnpm_unchanged_shape() {
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: true,
            workspace: None,
        };
        let out = render(n);
        assert!(out.contains("FROM node:22-bookworm-slim"));
        assert!(out.contains("pnpm install --frozen-lockfile"));
        assert!(out.contains("CMD [\"sh\",\"-c\",\"pnpm start\"]"));
        assert!(!out.contains("--filter"));
    }

    #[test]
    fn workspace_pnpm_uses_path_filter() {
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: true,
            workspace: Some(workspace(WorkspaceKind::Pnpm, "apps/web", Some("web"))),
        };
        let out = render(n);
        assert!(out.contains("pnpm --filter ./apps/web run build"));
        assert!(out.contains("CMD [\"sh\",\"-c\",\"pnpm --filter ./apps/web run start\"]"));
        assert!(out.contains("COPY pnpm-lock.yaml*"));
        assert!(out.contains("COPY pnpm-workspace.yaml*"));
    }

    #[test]
    fn workspace_bun_uses_path_filter() {
        let n = Node {
            node_major: 22,
            pm: Pm::Bun,
            has_lock: true,
            has_start_script: true,
            workspace: Some(workspace(
                WorkspaceKind::NpmYarnBun,
                "apps/api",
                Some("api"),
            )),
        };
        let out = render(n);
        assert!(out.contains("bun --filter ./apps/api"));
        assert!(out.contains("COPY bun.lockb*"));
        assert!(out.contains("https://bun.sh/install"));
    }

    #[test]
    fn workspace_npm_uses_w_flag() {
        let n = Node {
            node_major: 20,
            pm: Pm::Npm,
            has_lock: true,
            has_start_script: true,
            workspace: Some(workspace(
                WorkspaceKind::NpmYarnBun,
                "packages/jobs",
                Some("jobs"),
            )),
        };
        let out = render(n);
        assert!(out.contains("npm -w packages/jobs run build"));
    }

    #[test]
    fn workspace_yarn_requires_name() {
        let n = Node {
            node_major: 22,
            pm: Pm::Yarn,
            has_lock: true,
            has_start_script: true,
            workspace: Some(workspace(WorkspaceKind::NpmYarnBun, "apps/web", None)),
        };
        let err = n
            .render_dockerfile(&CommandOverrides::default())
            .unwrap_err();
        assert!(matches!(
            err,
            BuildError::StackRequiresField {
                stack: "node-workspace-yarn",
                ..
            }
        ));
    }

    #[test]
    fn workspace_yarn_uses_name() {
        let n = Node {
            node_major: 22,
            pm: Pm::Yarn,
            has_lock: true,
            has_start_script: true,
            workspace: Some(workspace(
                WorkspaceKind::NpmYarnBun,
                "apps/web",
                Some("@org/web"),
            )),
        };
        let out = render(n);
        assert!(out.contains("yarn workspace @org/web run start"));
    }

    #[test]
    fn workspace_filter_rejects_path_traversal() {
        let ws = workspace(WorkspaceKind::Pnpm, "../etc", Some("evil"));
        assert!(workspace_filter_token(Pm::Pnpm, &ws).is_err());
    }
}
