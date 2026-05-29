use crate::stack::{CommandOverrides, StackBuilder, StackDetector};
use crate::validate::{self, BuildError};
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug)]
pub struct Go {
    go_minor: u8,
}

impl StackDetector for Go {
    fn detect(source_dir: &Path) -> Option<Box<dyn StackBuilder>> {
        let go_mod = source_dir.join("go.mod");
        let raw = std::fs::read_to_string(&go_mod).ok()?;
        let go_minor = extract_go_minor(&raw).unwrap_or(22);
        Some(Box::new(Go { go_minor }))
    }
}

impl StackBuilder for Go {
    fn name(&self) -> &'static str {
        "go"
    }

    fn render_dockerfile(&self, ov: &CommandOverrides<'_>) -> Result<String, BuildError> {
        let default_build = "CGO_ENABLED=0 go build -o /out/server ./...";
        let default_start = "exec /server";

        let build_raw = ov.build_command.unwrap_or(default_build);
        let start_raw = ov.start_command.unwrap_or(default_start);

        let build_quoted = validate::shell_single_quote(build_raw, "build_command")?;
        let build_run = crate::stack::build_run_with_env(&build_quoted);
        let start_json = validate::cmd_to_json_token(start_raw, "start_command")?;
        let go = format!("1.{}", self.go_minor);

        Ok(format!(
            "# syntax=docker/dockerfile:1.7\n\
             FROM golang:{go}-bookworm AS build\n\
             WORKDIR /src\n\
             COPY . .\n\
             {build_run}\n\
             \n\
             FROM debian:bookworm-slim\n\
             RUN apt-get update \\\n\
                 && apt-get install -y --no-install-recommends ca-certificates \\\n\
                 && rm -rf /var/lib/apt/lists/*\n\
             COPY --from=build /out/server /server\n\
             ENV PORT=8080\n\
             EXPOSE 8080\n\
             CMD [\"sh\",\"-c\",{start_json}]\n"
        ))
    }
}

fn extract_go_minor(text: &str) -> Option<u8> {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = R.get_or_init(|| Regex::new(r"(?m)^\s*go\s+1\.(\d{1,3})(?:\.\d+)?\s*$").unwrap());
    let c = r.captures(text)?;
    let m = c.get(1)?;
    let n: u8 = m.as_str().parse().ok()?;
    let (_, minor) = validate::parse_go_minor(&format!("1.{n}")).ok()?;
    Some(minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple() {
        assert_eq!(extract_go_minor("module foo\n\ngo 1.22\n"), Some(22));
    }

    #[test]
    fn extracts_patch_version() {
        assert_eq!(extract_go_minor("module foo\n\ngo 1.22.3\n"), Some(22));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(extract_go_minor("module foo\n\ngo X.Y\n"), None);
    }
}
