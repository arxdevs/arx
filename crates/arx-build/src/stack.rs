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

pub fn detect_stack(source_dir: &Path) -> Option<Box<dyn StackBuilder>> {
    let detectors: &[DetectFn] = &[
        crate::stacks::Gradle::detect,
        crate::stacks::Maven::detect,
        crate::stacks::Node::detect,
        crate::stacks::Python::detect,
        crate::stacks::Go::detect,
    ];
    detectors.iter().find_map(|f| f(source_dir))
}
