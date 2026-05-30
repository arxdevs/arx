use crate::validate::BuildError;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct CommandOverrides<'a> {
    pub build_command: Option<&'a str>,
    pub start_command: Option<&'a str>,
}

pub trait StackBuilder: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &'static str;
    fn render_dockerfile(&self, overrides: &CommandOverrides<'_>) -> Result<String, BuildError>;
}

pub trait StackDetector {
    fn detect(source_dir: &Path) -> Option<Box<dyn StackBuilder>>;
}

type DetectFn = fn(&Path) -> Option<Box<dyn StackBuilder>>;

/// Render the build-stage `RUN` that mounts the service env as a BuildKit
/// secret and sources it before running the (already shell-escaped) build
/// command. `required=false` keeps builds working when no variables are set,
/// and referencing `$ARX_ENV_HASH` ties the layer cache to the env contents so
/// a changed variable forces a rebuild. `build_quoted` must already be escaped
/// via [`crate::validate::shell_single_quote`].
pub fn build_run_with_env(build_quoted: &str) -> String {
    format!(
        "ARG ARX_ENV_HASH=none\n\
         RUN --mount=type=secret,id=arx_env,required=false sh -c '. /run/secrets/arx_env 2>/dev/null || true; : \"$ARX_ENV_HASH\"; {build_quoted}'"
    )
}

pub fn detect_stack(source_dir: &Path) -> Option<Box<dyn StackBuilder>> {
    let detectors: &[DetectFn] = &[
        crate::stacks::Gradle::detect,
        crate::stacks::Maven::detect,
        crate::stacks::Node::detect,
        crate::stacks::Python::detect,
        crate::stacks::Go::detect,
        crate::stacks::Rust::detect,
    ];
    detectors.iter().find_map(|f| f(source_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_run_mounts_secret_and_sources_env() {
        let out = build_run_with_env("npm run build");
        assert!(out.contains("--mount=type=secret,id=arx_env,required=false"));
        assert!(out.contains(". /run/secrets/arx_env"));
        assert!(out.contains("ARG ARX_ENV_HASH=none"));
        assert!(out.contains("$ARX_ENV_HASH")); // referenced -> cache-bust on change
        assert!(out.contains("npm run build"));
    }
}
