use crate::stack::{CommandOverrides, StackBuilder, StackDetector};
use crate::validate::{self, BuildError};
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pm {
    Npm,
    Pnpm,
    Yarn,
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
        }
    }
    fn run_cmd(self) -> &'static str {
        match self {
            Pm::Npm => "npm start",
            Pm::Pnpm => "pnpm start",
            Pm::Yarn => "yarn start",
        }
    }
}

#[derive(Debug)]
pub struct Node {
    node_major: u8,
    pm: Pm,
    has_lock: bool,
    has_start_script: bool,
}

impl StackDetector for Node {
    fn detect(source_dir: &Path) -> Option<Box<dyn StackBuilder>> {
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
        }))
    }
}

impl StackBuilder for Node {
    fn name(&self) -> &'static str {
        "node"
    }

    fn render_dockerfile(&self, ov: &CommandOverrides<'_>) -> Result<String, BuildError> {
        let default_build = self.pm.install_cmd(self.has_lock);
        let default_start = if self.has_start_script {
            self.pm.run_cmd().to_string()
        } else {
            "node index.js".to_string()
        };

        let build_raw = ov.build_command.unwrap_or(default_build);
        let start_raw = ov.start_command.unwrap_or(default_start.as_str());

        let build_quoted = validate::shell_single_quote(build_raw, "build_command")?;
        let start_json = validate::cmd_to_json_token(start_raw, "start_command")?;
        let node_major = self.node_major;

        let pm_install = match self.pm {
            Pm::Npm => "",
            Pm::Pnpm => "RUN corepack enable && corepack prepare pnpm@latest --activate\n",
            Pm::Yarn => "RUN corepack enable && corepack prepare yarn@stable --activate\n",
        };

        Ok(format!(
            "# syntax=docker/dockerfile:1.7\n\
             FROM node:{node_major}-bookworm-slim\n\
             WORKDIR /app\n\
             {pm_install}\
             COPY . .\n\
             RUN sh -c '{build_quoted}'\n\
             ENV PORT=8080\n\
             EXPOSE 8080\n\
             CMD [\"sh\",\"-c\",{start_json}]\n"
        ))
    }
}

fn detect_pm(source_dir: &Path) -> (Pm, bool) {
    if source_dir.join("pnpm-lock.yaml").exists() {
        return (Pm::Pnpm, true);
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
