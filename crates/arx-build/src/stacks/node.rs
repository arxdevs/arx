use crate::monorepo::{WorkspaceContext, WorkspaceKind};
use crate::stack::{CommandOverrides, StackBuilder, StackDetector};
use crate::validate::{self, BuildError};
use serde_json::Value;
use std::path::Path;

const STATIC_DIST_DIR: &str = "dist";

/// `printf` format string for the SPA nginx server block. It is single-quoted
/// inside the generated `RUN`, so `${PORT}` and `$uri` reach the file verbatim;
/// the nginx image's envsubst entrypoint then substitutes only defined env vars
/// (`PORT`), leaving `$uri` for nginx itself.
const NGINX_SPA_TEMPLATE: &str = "server {\\n  listen ${PORT};\\n  root /usr/share/nginx/html;\\n  index index.html;\\n  location / {\\n    try_files $uri $uri/ /index.html;\\n  }\\n}\\n";

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

    fn run_build_cmd(self) -> &'static str {
        match self {
            Pm::Npm => "npm run build",
            Pm::Pnpm => "pnpm run build",
            Pm::Yarn => "yarn run build",
            Pm::Bun => "bun run build",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Pm::Npm => "npm",
            Pm::Pnpm => "pnpm",
            Pm::Yarn => "yarn",
            Pm::Bun => "bun",
        }
    }

    /// Pinned corepack default when the repo has no `packageManager` field.
    /// Never `latest` — that is the source of non-deterministic builds.
    fn corepack_default(self) -> Option<&'static str> {
        match self {
            Pm::Pnpm => Some("pnpm@10"),
            Pm::Yarn => Some("yarn@stable"),
            Pm::Npm | Pm::Bun => None,
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
    has_build_script: bool,
    uses_vite: bool,
    /// Sanitized `packageManager` spec (e.g. `pnpm@9.0.0`) when the repo pins
    /// one. Honored over the pinned default so builds match the lockfile's PM.
    pm_spec: Option<String>,
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
        // The monorepo root's `packageManager` is authoritative for workspaces.
        if let Some(spec) = read_package_manager(dir) {
            self.pm_spec = Some(spec);
        }
        self
    }

    /// corepack section honoring the repo's `packageManager` (when it matches
    /// the detected PM), else a pinned default. Disables corepack's interactive
    /// download prompt so the pinned/declared version is fetched non-interactively.
    fn corepack_section(&self) -> String {
        let prefix = format!("{}@", self.pm.name());
        let spec = self
            .pm_spec
            .as_deref()
            .filter(|s| s.starts_with(&prefix))
            .map(str::to_string)
            .or_else(|| self.pm.corepack_default().map(str::to_string));
        match spec {
            Some(s) => format!(
                "ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0\n\
                 RUN corepack enable && corepack prepare {s} --activate\n"
            ),
            None => String::new(),
        }
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
        let scripts = pkg.get("scripts");
        let has_start_script = scripts.and_then(|v| v.get("start")).is_some();
        let has_build_script = scripts.and_then(|v| v.get("build")).is_some();
        let build_script = scripts
            .and_then(|v| v.get("build"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let uses_vite = ["dependencies", "devDependencies"]
            .iter()
            .any(|k| pkg.get(k).and_then(|d| d.get("vite")).is_some())
            || build_script.contains("vite build");
        let pm_spec = pkg
            .get("packageManager")
            .and_then(|v| v.as_str())
            .and_then(validate::parse_package_manager);

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
            has_build_script,
            uses_vite,
            pm_spec,
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
        if self.is_static_site(ov) {
            return self.render_static(ov);
        }
        match &self.workspace {
            None => self.render_single_app(ov),
            Some(ws) => self.render_workspace(ov, ws),
        }
    }
}

impl Node {
    /// A package builds to a static site when it has a Vite build but nothing
    /// to start: no `start` script and no `start_command` override. Setting a
    /// `start_command` on the service always opts back into the dynamic path.
    fn is_static_site(&self, ov: &CommandOverrides<'_>) -> bool {
        if ov.start_command.is_some() || self.has_start_script {
            return false;
        }
        if !self.uses_vite {
            return false;
        }
        self.has_build_script || ov.build_command.is_some()
    }

    /// Dependency layer + sources for a workspace build: copy only the
    /// workspace manifests and lockfile, install once into a cached layer, then
    /// copy the full tree. Falls back to copy-all when manifests can't be
    /// enumerated — a partial set would break `--frozen-lockfile`.
    fn workspace_dependency_layer(&self, ws: &WorkspaceContext, install_quoted: &str) -> String {
        match manifest_copy_lines(&ws.workspace_manifests) {
            Some(manifests) => {
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
                format!(
                    "COPY package.json ./\n\
                     {lockfile_line}\
                     {workspace_meta_line}\
                     {manifests}\
                     RUN sh -c '{install_quoted}'\n\
                     COPY . .\n"
                )
            }
            None => format!(
                "COPY . .\n\
                 RUN sh -c '{install_quoted}'\n"
            ),
        }
    }

    /// Two-stage build: the usual Node/workspace build stage, then an
    /// nginx:1-alpine runtime serving the Vite `dist/` output with an SPA
    /// fallback. `listen ${PORT}` goes through the nginx image's envsubst
    /// templates so the runtime port follows the service `PORT` variable,
    /// matching the TCP healthcheck and traefik routing.
    fn render_static(&self, ov: &CommandOverrides<'_>) -> Result<String, BuildError> {
        let node_major = self.node_major;
        let corepack = self.corepack_section();
        let bun_install = self.pm.base_image_extra();
        let install_cmd = self.pm.install_cmd(self.has_lock);
        let install_quoted = validate::shell_single_quote(install_cmd, "build_command")?;

        let (default_build, dist_src, sources) = match &self.workspace {
            Some(ws) => {
                let filter = workspace_filter_token(self.pm, ws)?;
                let path = ws.package_rel_path.as_str();
                if !validate::is_safe_copy_path(path) {
                    return Err(BuildError::InvalidInput {
                        field: "workspace_package_path",
                        reason: "invalid package path".into(),
                    });
                }
                (
                    format!("{filter} run build"),
                    format!("/app/{path}/{STATIC_DIST_DIR}"),
                    self.workspace_dependency_layer(ws, &install_quoted),
                )
            }
            None => (
                self.pm.run_build_cmd().to_string(),
                format!("/app/{STATIC_DIST_DIR}"),
                format!(
                    "COPY . .\n\
                     RUN sh -c '{install_quoted}'\n"
                ),
            ),
        };

        let build_raw = ov.build_command.unwrap_or(default_build.as_str());
        let build_quoted = validate::shell_single_quote(build_raw, "build_command")?;
        let build_run = crate::stack::build_run_with_env(&build_quoted);

        Ok(format!(
            "# syntax=docker/dockerfile:1.7\n\
             FROM node:{node_major}-bookworm-slim AS build\n\
             WORKDIR /app\n\
             {bun_install}\
             {corepack}\
             {sources}\
             {build_run}\n\
             \n\
             FROM nginx:1-alpine\n\
             COPY --from=build [\"{dist_src}\", \"/usr/share/nginx/html\"]\n\
             RUN mkdir -p /etc/nginx/templates \\\n\
                 && printf '{NGINX_SPA_TEMPLATE}' > /etc/nginx/templates/default.conf.template\n\
             ENV PORT=8080\n\
             EXPOSE 8080\n"
        ))
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

        let corepack = self.corepack_section();
        let bun_install = self.pm.base_image_extra();

        Ok(format!(
            "# syntax=docker/dockerfile:1.7\n\
             FROM node:{node_major}-bookworm-slim\n\
             WORKDIR /app\n\
             {bun_install}\
             {corepack}\
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
        // Install runs once in its own layer (below); the build step only builds.
        let default_build = format!("{filter} run build");
        let default_start = format!("{filter} run start");

        let build_raw = ov.build_command.unwrap_or(default_build.as_str());
        let start_raw = ov.start_command.unwrap_or(default_start.as_str());

        let build_quoted = validate::shell_single_quote(build_raw, "build_command")?;
        let build_run = crate::stack::build_run_with_env(&build_quoted);
        let start_json = validate::cmd_to_json_token(start_raw, "start_command")?;
        let install_quoted = validate::shell_single_quote(install_cmd, "build_command")?;
        let node_major = self.node_major;
        let corepack = self.corepack_section();
        let bun_install = self.pm.base_image_extra();

        let header = format!(
            "# syntax=docker/dockerfile:1.7\n\
             FROM node:{node_major}-bookworm-slim\n\
             WORKDIR /app\n\
             {bun_install}\
             {corepack}"
        );
        let footer = format!(
            "ENV PORT=8080\n\
             EXPOSE 8080\n\
             CMD [\"sh\",\"-c\",{start_json}]\n"
        );

        // Dependency layer: copy only the workspace manifests + lockfile, then a
        // single install. This caches across source-only changes and avoids the
        // double-install (root-only pre-install + full install) that corrupts
        // pnpm's store and empties node_modules.
        let sources = self.workspace_dependency_layer(ws, &install_quoted);

        Ok(format!("{header}{sources}{build_run}\n{footer}"))
    }
}

/// COPY lines (exec form) for each workspace `package.json`. Returns `None` to
/// trigger the copy-all fallback: empty list, or any path unsafe to embed.
fn manifest_copy_lines(manifests: &[String]) -> Option<String> {
    if manifests.is_empty() {
        return None;
    }
    let mut s = String::new();
    for p in manifests {
        if !validate::is_safe_copy_path(p) {
            return None;
        }
        s.push_str(&format!("COPY [\"{p}\", \"{p}\"]\n"));
    }
    Some(s)
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

/// Read and sanitize the `packageManager` field from a directory's package.json.
fn read_package_manager(dir: &Path) -> Option<String> {
    let raw = read_capped(&dir.join("package.json"), 256 * 1024).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    v.get("packageManager")
        .and_then(|x| x.as_str())
        .and_then(validate::parse_package_manager)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(kind: WorkspaceKind, path: &str, name: Option<&str>) -> WorkspaceContext {
        WorkspaceContext {
            kind,
            package_rel_path: path.to_string(),
            package_name: name.map(String::from),
            workspace_manifests: vec![format!("{path}/package.json")],
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
            has_build_script: true,
            uses_vite: false,
            pm_spec: None,
            workspace: None,
        };
        let out = render(n);
        assert!(out.contains("FROM node:22-bookworm-slim"));
        assert!(out.contains("pnpm install --frozen-lockfile"));
        assert!(out.contains("CMD [\"sh\",\"-c\",\"pnpm start\"]"));
        assert!(!out.contains("--filter"));
    }

    #[test]
    fn static_spa_single_app_renders_nginx_stage() {
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: false,
            has_build_script: true,
            uses_vite: true,
            pm_spec: None,
            workspace: None,
        };
        let out = render(n);
        assert!(out.contains("FROM node:22-bookworm-slim AS build"));
        assert!(out.contains("pnpm run build"));
        assert!(out.contains("--mount=type=secret,id=arx_env"));
        assert!(out.contains("FROM nginx:1-alpine"));
        assert!(out.contains("COPY --from=build [\"/app/dist\", \"/usr/share/nginx/html\"]"));
        assert!(out.contains("/etc/nginx/templates/default.conf.template"));
        assert!(out.contains("listen ${PORT};"));
        assert!(out.contains("try_files $uri $uri/ /index.html;"));
        assert!(out.contains("ENV PORT=8080"));
        assert!(!out.contains("CMD ["));
    }

    #[test]
    fn static_spa_workspace_builds_package_and_copies_its_dist() {
        let n = Node {
            node_major: 24,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: false,
            has_build_script: true,
            uses_vite: true,
            pm_spec: None,
            workspace: Some(workspace(WorkspaceKind::Pnpm, "apps/kiosk", Some("kiosk"))),
        };
        let out = render(n);
        assert!(out.contains("pnpm --filter ./apps/kiosk run build"));
        assert!(
            out.contains("COPY --from=build [\"/app/apps/kiosk/dist\", \"/usr/share/nginx/html\"]")
        );
        assert!(out.contains("COPY pnpm-lock.yaml*"));
        assert!(out.contains("COPY pnpm-workspace.yaml*"));
        assert_eq!(out.matches("pnpm install --frozen-lockfile").count(), 1);
        assert!(!out.contains("run start"));
    }

    #[test]
    fn static_spa_respects_build_command_override() {
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: false,
            has_build_script: true,
            uses_vite: true,
            pm_spec: None,
            workspace: Some(workspace(WorkspaceKind::Pnpm, "apps/web", Some("web"))),
        };
        let out = n
            .render_dockerfile(&CommandOverrides {
                build_command: Some("pnpm turbo build --filter=web"),
                start_command: None,
            })
            .unwrap();
        assert!(out.contains("pnpm turbo build --filter=web"));
        assert!(out.contains("FROM nginx:1-alpine"));
    }

    #[test]
    fn start_command_override_opts_out_of_static() {
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: false,
            has_build_script: true,
            uses_vite: true,
            pm_spec: None,
            workspace: Some(workspace(WorkspaceKind::Pnpm, "apps/web", Some("web"))),
        };
        let out = n
            .render_dockerfile(&CommandOverrides {
                build_command: None,
                start_command: Some("npx serve dist"),
            })
            .unwrap();
        assert!(!out.contains("FROM nginx"));
        assert!(out.contains("CMD [\"sh\",\"-c\",\"npx serve dist\"]"));
    }

    #[test]
    fn start_script_keeps_dynamic_path_even_with_vite() {
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: true,
            has_build_script: true,
            uses_vite: true,
            pm_spec: None,
            workspace: None,
        };
        let out = render(n);
        assert!(!out.contains("FROM nginx"));
        assert!(out.contains("CMD [\"sh\",\"-c\",\"pnpm start\"]"));
    }

    #[test]
    fn no_vite_no_start_stays_dynamic() {
        let n = Node {
            node_major: 22,
            pm: Pm::Npm,
            has_lock: true,
            has_start_script: false,
            has_build_script: true,
            uses_vite: false,
            pm_spec: None,
            workspace: None,
        };
        let out = render(n);
        assert!(!out.contains("FROM nginx"));
        assert!(out.contains("CMD [\"sh\",\"-c\",\"node index.js\"]"));
    }

    #[test]
    fn workspace_pnpm_uses_path_filter() {
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: true,
            has_build_script: true,
            uses_vite: false,
            pm_spec: None,
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
            has_build_script: true,
            uses_vite: false,
            pm_spec: None,
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
            has_build_script: true,
            uses_vite: false,
            pm_spec: None,
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
            has_build_script: true,
            uses_vite: false,
            pm_spec: None,
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
            has_build_script: true,
            uses_vite: false,
            pm_spec: None,
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

    #[test]
    fn corepack_honors_package_manager_field() {
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: true,
            has_build_script: true,
            uses_vite: false,
            pm_spec: Some("pnpm@9.0.0".to_string()),
            workspace: Some(workspace(WorkspaceKind::Pnpm, "apps/web", Some("web"))),
        };
        let out = render(n);
        assert!(out.contains("corepack prepare pnpm@9.0.0 --activate"));
        assert!(out.contains("ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0"));
        assert!(!out.contains("pnpm@latest"));
    }

    #[test]
    fn corepack_pins_default_without_package_manager() {
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: true,
            has_build_script: true,
            uses_vite: false,
            pm_spec: None,
            workspace: Some(workspace(WorkspaceKind::Pnpm, "apps/web", Some("web"))),
        };
        let out = render(n);
        assert!(out.contains("corepack prepare pnpm@10 --activate"));
        assert!(!out.contains("pnpm@latest"));
    }

    #[test]
    fn corepack_ignores_mismatched_package_manager() {
        // packageManager says yarn but the detected PM is pnpm -> use pnpm default.
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: true,
            has_build_script: true,
            uses_vite: false,
            pm_spec: Some("yarn@4.1.0".to_string()),
            workspace: Some(workspace(WorkspaceKind::Pnpm, "apps/web", Some("web"))),
        };
        let out = render(n);
        assert!(out.contains("corepack prepare pnpm@10 --activate"));
        assert!(!out.contains("yarn@4.1.0"));
    }

    #[test]
    fn workspace_installs_once_and_copies_manifests() {
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: true,
            has_build_script: true,
            uses_vite: false,
            pm_spec: None,
            workspace: Some(WorkspaceContext {
                kind: WorkspaceKind::Pnpm,
                package_rel_path: "apps/web".to_string(),
                package_name: Some("web".to_string()),
                workspace_manifests: vec![
                    "apps/web/package.json".to_string(),
                    "packages/ui/package.json".to_string(),
                ],
            }),
        };
        let out = render(n);
        assert!(out.contains("COPY [\"apps/web/package.json\", \"apps/web/package.json\"]"));
        assert!(out.contains("COPY [\"packages/ui/package.json\", \"packages/ui/package.json\"]"));
        // single install (dependency layer); the old speculative pre-install
        // (`<install>' || true`) is gone.
        assert!(!out.contains("--frozen-lockfile' || true"));
        assert_eq!(out.matches("pnpm install --frozen-lockfile").count(), 1);
        // build step builds only, secret mount preserved
        assert!(out.contains("pnpm --filter ./apps/web run build"));
        assert!(out.contains("--mount=type=secret,id=arx_env"));
    }

    #[test]
    fn workspace_falls_back_to_copy_all_without_manifests() {
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: true,
            has_build_script: true,
            uses_vite: false,
            pm_spec: None,
            workspace: Some(WorkspaceContext {
                kind: WorkspaceKind::Pnpm,
                package_rel_path: "apps/web".to_string(),
                package_name: Some("web".to_string()),
                workspace_manifests: vec![],
            }),
        };
        let out = render(n);
        // no manifest dependency layer
        assert!(!out.contains("COPY pnpm-lock.yaml*"));
        assert!(!out.contains("COPY ["));
        // still a single correct install over the full tree
        assert!(out.contains("COPY . ."));
        assert_eq!(out.matches("pnpm install --frozen-lockfile").count(), 1);
        assert!(!out.contains("--frozen-lockfile' || true"));
    }

    #[test]
    fn unsafe_manifest_path_triggers_copy_all_fallback() {
        let n = Node {
            node_major: 22,
            pm: Pm::Pnpm,
            has_lock: true,
            has_start_script: true,
            has_build_script: true,
            uses_vite: false,
            pm_spec: None,
            workspace: Some(WorkspaceContext {
                kind: WorkspaceKind::Pnpm,
                package_rel_path: "apps/web".to_string(),
                package_name: Some("web".to_string()),
                workspace_manifests: vec!["../evil/package.json".to_string()],
            }),
        };
        let out = render(n);
        assert!(!out.contains("COPY ["));
        assert!(out.contains("COPY . ."));
    }
}
