use crate::stack::{CommandOverrides, StackBuilder, StackDetector};
use crate::validate::{self, BuildError};
use std::path::Path;

#[derive(Debug)]
pub struct Rust {
    /// Validated package/binary name; lands in the `COPY` source path.
    bin: String,
    /// Build image tag, e.g. `1` (latest stable 1.x) or `1.85`.
    rust_tag: String,
}

impl StackDetector for Rust {
    fn detect(source_dir: &Path) -> Option<Box<dyn StackBuilder>> {
        let raw = std::fs::read_to_string(source_dir.join("Cargo.toml")).ok()?;
        let doc: toml::Value = toml::from_str(&raw).ok()?;

        // A single binary name is required to fix the `COPY` path. A virtual
        // workspace manifest (no `[package]`, no `[[bin]]`) yields None here.
        let bin = validate::validate_cargo_name(resolve_bin_name(&doc)?)
            .ok()?
            .to_string();

        // Only claim the repo if a runnable binary is plausible. A lib-only
        // crate falls through so the user gets a clear NoStack message.
        let has_bin_array = doc
            .get("bin")
            .and_then(|b| b.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        let has_main = source_dir.join("src/main.rs").exists();
        let has_src_bin = std::fs::read_dir(source_dir.join("src/bin"))
            .map(|mut it| it.next().is_some())
            .unwrap_or(false);
        if !(has_bin_array || has_main || has_src_bin) {
            return None;
        }

        Some(Box::new(Rust {
            bin,
            rust_tag: resolve_rust_tag(&doc),
        }))
    }
}

impl StackBuilder for Rust {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn render_dockerfile(&self, ov: &CommandOverrides<'_>) -> Result<String, BuildError> {
        // `--release` only; no `--locked` so repos without a committed
        // Cargo.lock still build. Users override when they need otherwise.
        let default_build = "cargo build --release";
        let default_start = "exec /server";

        let build_raw = ov.build_command.unwrap_or(default_build);
        let start_raw = ov.start_command.unwrap_or(default_start);

        let build_quoted = validate::shell_single_quote(build_raw, "build_command")?;
        let build_run = crate::stack::build_run_with_env(&build_quoted);
        let start_json = validate::cmd_to_json_token(start_raw, "start_command")?;
        let ver = &self.rust_tag;
        let bin = &self.bin;

        Ok(format!(
            "# syntax=docker/dockerfile:1.7\n\
             FROM rust:{ver}-bookworm AS build\n\
             WORKDIR /src\n\
             COPY . .\n\
             {build_run}\n\
             \n\
             FROM debian:bookworm-slim\n\
             RUN apt-get update \\\n\
                 && apt-get install -y --no-install-recommends ca-certificates \\\n\
                 && rm -rf /var/lib/apt/lists/*\n\
             COPY --from=build /src/target/release/{bin} /server\n\
             ENV PORT=8080\n\
             EXPOSE 8080\n\
             CMD [\"sh\",\"-c\",{start_json}]\n"
        ))
    }
}

/// Prefer an explicit `[[bin]]` name, else fall back to `[package].name`.
fn resolve_bin_name(doc: &toml::Value) -> Option<&str> {
    let from_array = doc
        .get("bin")
        .and_then(|b| b.as_array())
        .and_then(|a| a.first())
        .and_then(|t| t.get("name"))
        .and_then(|v| v.as_str());
    let from_pkg = doc
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|v| v.as_str());
    from_array.or(from_pkg)
}

/// `[package].rust-version` pins the build image; otherwise track latest 1.x.
fn resolve_rust_tag(doc: &toml::Value) -> String {
    doc.get("package")
        .and_then(|p| p.get("rust-version"))
        .and_then(|v| v.as_str())
        .and_then(|s| validate::parse_rust_minor(s).ok())
        .map(|(_, minor)| format!("1.{minor}"))
        .unwrap_or_else(|| "1".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_package_name() {
        let doc = toml::from_str("[package]\nname = \"my-app\"\n").unwrap();
        assert_eq!(resolve_bin_name(&doc), Some("my-app"));
    }

    #[test]
    fn bin_array_overrides_package() {
        let doc = toml::from_str("[package]\nname = \"pkg\"\n\n[[bin]]\nname = \"cli\"\n").unwrap();
        assert_eq!(resolve_bin_name(&doc), Some("cli"));
    }

    #[test]
    fn virtual_workspace_has_no_bin() {
        let doc = toml::from_str("[workspace]\nmembers = [\"a\"]\n").unwrap();
        assert_eq!(resolve_bin_name(&doc), None);
    }

    #[test]
    fn rust_tag_defaults_and_pins() {
        let none = toml::from_str("[package]\nname = \"x\"\n").unwrap();
        assert_eq!(resolve_rust_tag(&none), "1");
        let pinned = toml::from_str("[package]\nname = \"x\"\nrust-version = \"1.85\"\n").unwrap();
        assert_eq!(resolve_rust_tag(&pinned), "1.85");
    }

    #[test]
    fn render_contains_expected() {
        let r = Rust {
            bin: "my-app".into(),
            rust_tag: "1".into(),
        };
        let out = r.render_dockerfile(&CommandOverrides::default()).unwrap();
        assert!(out.contains("FROM rust:1-bookworm AS build"));
        assert!(out.contains("/src/target/release/my-app /server"));
        assert!(out.contains("cargo build --release"));
        assert!(out.contains("EXPOSE 8080"));
    }
}
