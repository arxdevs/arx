use crate::stack::{CommandOverrides, StackBuilder, StackDetector};
use crate::validate::{self, BuildError};
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug)]
pub struct Gradle {
    jdk: u8,
    spring_boot: bool,
}

impl StackDetector for Gradle {
    fn detect(source_dir: &Path) -> Option<Box<dyn StackBuilder>> {
        let candidates = ["build.gradle.kts", "build.gradle"];
        let mut text = String::new();
        for name in candidates {
            let p = source_dir.join(name);
            if let Ok(s) = read_capped(&p, 512 * 1024) {
                text.push_str(&s);
                text.push('\n');
            }
        }
        if text.is_empty() {
            return None;
        }
        let jdk = extract_jdk(&text).unwrap_or(21);
        let spring_boot = text.contains("org.springframework.boot");
        Some(Box::new(Gradle { jdk, spring_boot }))
    }
}

impl StackBuilder for Gradle {
    fn name(&self) -> &'static str {
        "gradle"
    }

    fn render_dockerfile(&self, ov: &CommandOverrides<'_>) -> Result<String, BuildError> {
        let default_build = if self.spring_boot {
            "./gradlew bootJar -x test --no-daemon"
        } else {
            "./gradlew build -x test --no-daemon"
        };
        let default_start = "exec java -jar -Dserver.port=${PORT:-8080} /app/app.jar";

        let build_raw = ov.build_command.unwrap_or(default_build);
        let start_raw = ov.start_command.unwrap_or(default_start);

        let build_quoted = validate::shell_single_quote(build_raw, "build_command")?;
        let start_json = validate::cmd_to_json_token(start_raw, "start_command")?;
        let jdk = self.jdk;

        Ok(format!(
            "# syntax=docker/dockerfile:1.7\n\
             FROM eclipse-temurin:{jdk}-jdk AS build\n\
             WORKDIR /app\n\
             COPY . .\n\
             RUN chmod +x gradlew 2>/dev/null || true\n\
             RUN sh -c '{build_quoted}'\n\
             \n\
             FROM eclipse-temurin:{jdk}-jre\n\
             WORKDIR /app\n\
             COPY --from=build /app/build/libs/*.jar /app/app.jar\n\
             ENV PORT=8080\n\
             EXPOSE 8080\n\
             CMD [\"sh\",\"-c\",{start_json}]\n"
        ))
    }
}

fn extract_jdk(text: &str) -> Option<u8> {
    static R1: OnceLock<Regex> = OnceLock::new();
    let r1 = R1.get_or_init(|| Regex::new(r"JavaLanguageVersion\.of\(\s*(\d{1,3})\s*\)").unwrap());
    if let Some(c) = r1.captures(text)
        && let Some(m) = c.get(1)
        && let Ok(n) = validate::parse_jdk_major(m.as_str())
    {
        return Some(n);
    }
    static R2: OnceLock<Regex> = OnceLock::new();
    let r2 = R2.get_or_init(|| {
        Regex::new(
            r"(?:sourceCompatibility|targetCompatibility)\s*=\s*JavaVersion\.VERSION_(\d{1,3})",
        )
        .unwrap()
    });
    if let Some(c) = r2.captures(text)
        && let Some(m) = c.get(1)
        && let Ok(n) = validate::parse_jdk_major(m.as_str())
    {
        return Some(n);
    }
    static R3: OnceLock<Regex> = OnceLock::new();
    let r3 = R3.get_or_init(|| {
        Regex::new(r#"(?:sourceCompatibility|targetCompatibility)\s*=\s*['"]?(\d{1,3})['"]?"#)
            .unwrap()
    });
    if let Some(c) = r3.captures(text)
        && let Some(m) = c.get(1)
        && let Ok(n) = validate::parse_jdk_major(m.as_str())
    {
        return Some(n);
    }
    None
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
    fn detects_toolchain() {
        let text = r#"
java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(21)
    }
}
        "#;
        assert_eq!(extract_jdk(text), Some(21));
    }

    #[test]
    fn detects_source_compat_javaversion() {
        assert_eq!(
            extract_jdk("sourceCompatibility = JavaVersion.VERSION_17"),
            Some(17)
        );
    }

    #[test]
    fn detects_source_compat_string() {
        assert_eq!(extract_jdk(r#"sourceCompatibility = '11'"#), Some(11));
        assert_eq!(extract_jdk("sourceCompatibility = 17"), Some(17));
    }

    #[test]
    fn rejects_injection_via_jdk_number() {
        assert_eq!(extract_jdk("JavaLanguageVersion.of(21) AS evil"), Some(21));
    }
}
