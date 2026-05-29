use crate::stack::{CommandOverrides, StackBuilder, StackDetector};
use crate::validate::{self, BuildError};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::path::Path;

#[derive(Debug)]
pub struct Maven {
    jdk: u8,
}

impl StackDetector for Maven {
    fn detect(source_dir: &Path) -> Option<Box<dyn StackBuilder>> {
        let p = source_dir.join("pom.xml");
        let text = read_capped(&p, 1024 * 1024).ok()?;
        let jdk = extract_jdk(&text).unwrap_or(21);
        Some(Box::new(Maven { jdk }))
    }
}

impl StackBuilder for Maven {
    fn name(&self) -> &'static str {
        "maven"
    }

    fn render_dockerfile(&self, ov: &CommandOverrides<'_>) -> Result<String, BuildError> {
        let default_build = "mvn -B -ntp package -DskipTests";
        let default_start = "exec java -jar -Dserver.port=${PORT:-8080} /app/target/*.jar";

        let build_raw = ov.build_command.unwrap_or(default_build);
        let start_raw = ov.start_command.unwrap_or(default_start);

        let build_quoted = validate::shell_single_quote(build_raw, "build_command")?;
        let build_run = crate::stack::build_run_with_env(&build_quoted);
        let start_json = validate::cmd_to_json_token(start_raw, "start_command")?;
        let jdk = self.jdk;

        Ok(format!(
            "# syntax=docker/dockerfile:1.7\n\
             FROM maven:3-eclipse-temurin-{jdk} AS build\n\
             WORKDIR /app\n\
             COPY . .\n\
             {build_run}\n\
             \n\
             FROM eclipse-temurin:{jdk}-jre\n\
             WORKDIR /app\n\
             COPY --from=build /app/target/*.jar /app/app.jar\n\
             ENV PORT=8080\n\
             EXPOSE 8080\n\
             CMD [\"sh\",\"-c\",{start_json}]\n"
        ))
    }
}

/// Event-based parse — never materialises a DOM the attacker controls.
fn extract_jdk(text: &str) -> Option<u8> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut current_tag: Option<String> = None;
    let mut found: Option<u8> = None;
    let interesting = [
        "maven.compiler.release",
        "maven.compiler.source",
        "java.version",
    ];
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_vec();
                let name = String::from_utf8_lossy(&name).into_owned();
                current_tag = Some(name);
            }
            Ok(Event::End(_)) => {
                current_tag = None;
            }
            Ok(Event::Text(t)) => {
                if let Some(tag) = &current_tag
                    && interesting.contains(&tag.as_str())
                    && let Ok(text) = t.unescape()
                    && let Ok(n) = validate::parse_jdk_major(text.trim())
                {
                    found = Some(n);
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    found
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

    #[test]
    fn extracts_release() {
        let pom = r#"<project>
            <properties>
                <maven.compiler.release>21</maven.compiler.release>
            </properties>
        </project>"#;
        assert_eq!(extract_jdk(pom), Some(21));
    }

    #[test]
    fn extracts_java_version_spring_style() {
        let pom = r#"<project>
            <properties>
                <java.version>17</java.version>
            </properties>
        </project>"#;
        assert_eq!(extract_jdk(pom), Some(17));
    }

    #[test]
    fn ignores_garbage() {
        let pom = r#"<project>
            <properties>
                <java.version>21 AS evil</java.version>
            </properties>
        </project>"#;
        assert!(extract_jdk(pom).is_none());
    }
}
