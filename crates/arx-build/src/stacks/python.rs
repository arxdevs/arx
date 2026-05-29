use crate::stack::{CommandOverrides, StackBuilder, StackDetector};
use crate::validate::{self, BuildError};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepStyle {
    Requirements,
    Poetry,
    PyprojectPip,
}

#[derive(Debug)]
pub struct Python {
    py_major: u8,
    py_minor: u8,
    style: DepStyle,
}

impl StackDetector for Python {
    fn detect(source_dir: &Path) -> Option<Box<dyn StackBuilder>> {
        let has_pyproject = source_dir.join("pyproject.toml").exists();
        let has_requirements = source_dir.join("requirements.txt").exists();
        if !has_pyproject && !has_requirements {
            return None;
        }
        let style = if source_dir.join("poetry.lock").exists() {
            DepStyle::Poetry
        } else if has_pyproject {
            DepStyle::PyprojectPip
        } else {
            DepStyle::Requirements
        };

        let from_pyenv = std::fs::read_to_string(source_dir.join(".python-version"))
            .ok()
            .and_then(|s| validate::parse_python_minor(s.trim()).ok());

        let from_pyproject = if has_pyproject {
            std::fs::read_to_string(source_dir.join("pyproject.toml"))
                .ok()
                .and_then(|t| extract_requires_python(&t))
        } else {
            None
        };

        let (py_major, py_minor) = from_pyenv.or(from_pyproject).unwrap_or((3, 12));

        Some(Box::new(Python {
            py_major,
            py_minor,
            style,
        }))
    }
}

impl StackBuilder for Python {
    fn name(&self) -> &'static str {
        "python"
    }

    fn render_dockerfile(&self, ov: &CommandOverrides<'_>) -> Result<String, BuildError> {
        let default_build = match self.style {
            DepStyle::Requirements => "pip install --no-cache-dir -r requirements.txt",
            DepStyle::Poetry => {
                "pip install --no-cache-dir poetry && poetry config virtualenvs.create false && poetry install --no-interaction --no-ansi --without dev"
            }
            DepStyle::PyprojectPip => "pip install --no-cache-dir .",
        };

        // Python has no universal entrypoint convention — require an explicit override.
        let start_raw = ov.start_command.ok_or(BuildError::StackRequiresField {
            stack: "python",
            field: "start_command",
        })?;

        let build_raw = ov.build_command.unwrap_or(default_build);
        let build_quoted = validate::shell_single_quote(build_raw, "build_command")?;
        let build_run = crate::stack::build_run_with_env(&build_quoted);
        let start_json = validate::cmd_to_json_token(start_raw, "start_command")?;
        let py = format!("{}.{}", self.py_major, self.py_minor);

        Ok(format!(
            "# syntax=docker/dockerfile:1.7\n\
             FROM python:{py}-slim\n\
             WORKDIR /app\n\
             COPY . .\n\
             {build_run}\n\
             ENV PORT=8080\n\
             EXPOSE 8080\n\
             CMD [\"sh\",\"-c\",{start_json}]\n"
        ))
    }
}

fn extract_requires_python(toml_text: &str) -> Option<(u8, u8)> {
    let doc: toml::Value = toml::from_str(toml_text).ok()?;
    let s = doc
        .get("project")
        .and_then(|p| p.get("requires-python"))
        .and_then(|v| v.as_str())?;
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ' ')
        .collect();
    for tok in cleaned.split_whitespace() {
        if let Ok(p) = validate::parse_python_minor(tok) {
            return Some(p);
        }
    }
    None
}
