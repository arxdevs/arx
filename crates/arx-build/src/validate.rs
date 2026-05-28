//! Every value that ends up inside a generated Dockerfile or a `docker`/`git`
//! argv passes through here first. Bypassing this module = injection risk.

use regex::Regex;
use std::sync::OnceLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("invalid input ({field}): {reason}")]
    InvalidInput { field: &'static str, reason: String },
    #[error(
        "no Dockerfile and no supported stack detected. \
             Provide a Dockerfile in the repo, or set build_command + start_command \
             on the service"
    )]
    NoStack,
    #[error("stack `{stack}` requires {field} to be set on the service")]
    StackRequiresField {
        stack: &'static str,
        field: &'static str,
    },
    #[error("io: {0}")]
    Io(String),
    #[error("docker build failed (exit {0})")]
    DockerBuildFailed(i32),
}

impl From<std::io::Error> for BuildError {
    fn from(e: std::io::Error) -> Self {
        BuildError::Io(e.to_string())
    }
}

const MAX_VERSION_LEN: usize = 16;
const MAX_CMD_LEN: usize = 8 * 1024;
const MAX_REPO_LEN: usize = 200;
const MAX_REF_LEN: usize = 200;

fn version_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\d{1,3}(?:\.\d{1,3}){0,2}$").unwrap())
}

fn major_only_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\d{1,3}$").unwrap())
}

pub fn parse_jdk_major(s: &str) -> Result<u8, BuildError> {
    let s = s.trim();
    if s.len() > MAX_VERSION_LEN {
        return Err(BuildError::InvalidInput {
            field: "jdk_version",
            reason: "too long".into(),
        });
    }
    if !major_only_re().is_match(s) {
        return Err(BuildError::InvalidInput {
            field: "jdk_version",
            reason: format!("not an integer 1..99: {s:?}"),
        });
    }
    let n: u8 = s.parse().map_err(|_| BuildError::InvalidInput {
        field: "jdk_version",
        reason: "parse".into(),
    })?;
    if !(1..=99).contains(&n) {
        return Err(BuildError::InvalidInput {
            field: "jdk_version",
            reason: "out of range".into(),
        });
    }
    Ok(n)
}

pub fn parse_node_major(s: &str) -> Result<u8, BuildError> {
    let s = s.trim().trim_start_matches('v');
    if s.len() > MAX_VERSION_LEN {
        return Err(BuildError::InvalidInput {
            field: "node_version",
            reason: "too long".into(),
        });
    }
    if !version_re().is_match(s) {
        return Err(BuildError::InvalidInput {
            field: "node_version",
            reason: format!("not a version: {s:?}"),
        });
    }
    let major: u8 =
        s.split('.')
            .next()
            .unwrap_or("")
            .parse()
            .map_err(|_| BuildError::InvalidInput {
                field: "node_version",
                reason: "parse".into(),
            })?;
    if !(1..=99).contains(&major) {
        return Err(BuildError::InvalidInput {
            field: "node_version",
            reason: "out of range".into(),
        });
    }
    Ok(major)
}

/// Returns None for shapes we can't reason about (e.g. `"latest"`).
pub fn parse_node_major_from_engines(spec: &str) -> Option<u8> {
    if let Ok(n) = parse_node_major(spec) {
        return Some(n);
    }
    let mut runs: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in spec.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    runs.iter().find_map(|r| parse_node_major(r).ok())
}

pub fn parse_python_minor(s: &str) -> Result<(u8, u8), BuildError> {
    let s = s.trim();
    if s.len() > MAX_VERSION_LEN {
        return Err(BuildError::InvalidInput {
            field: "python_version",
            reason: "too long".into(),
        });
    }
    if !version_re().is_match(s) {
        return Err(BuildError::InvalidInput {
            field: "python_version",
            reason: format!("not a version: {s:?}"),
        });
    }
    let mut parts = s.split('.');
    let major: u8 = parts
        .next()
        .ok_or_else(|| BuildError::InvalidInput {
            field: "python_version",
            reason: "missing major".into(),
        })?
        .parse()
        .map_err(|_| BuildError::InvalidInput {
            field: "python_version",
            reason: "parse major".into(),
        })?;
    let minor: u8 = parts
        .next()
        .unwrap_or("0")
        .parse()
        .map_err(|_| BuildError::InvalidInput {
            field: "python_version",
            reason: "parse minor".into(),
        })?;
    if !matches!(major, 3 | 4) || minor > 30 {
        return Err(BuildError::InvalidInput {
            field: "python_version",
            reason: "unsupported major.minor".into(),
        });
    }
    Ok((major, minor))
}

pub fn parse_go_minor(s: &str) -> Result<(u8, u8), BuildError> {
    let s = s.trim();
    if s.len() > MAX_VERSION_LEN {
        return Err(BuildError::InvalidInput {
            field: "go_version",
            reason: "too long".into(),
        });
    }
    if !version_re().is_match(s) {
        return Err(BuildError::InvalidInput {
            field: "go_version",
            reason: format!("not a version: {s:?}"),
        });
    }
    let mut parts = s.split('.');
    let major: u8 = parts
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| BuildError::InvalidInput {
            field: "go_version",
            reason: "parse major".into(),
        })?;
    let minor: u8 = parts
        .next()
        .unwrap_or("0")
        .parse()
        .map_err(|_| BuildError::InvalidInput {
            field: "go_version",
            reason: "parse minor".into(),
        })?;
    if major != 1 || !(18..=50).contains(&minor) {
        return Err(BuildError::InvalidInput {
            field: "go_version",
            reason: "unsupported".into(),
        });
    }
    Ok((major, minor))
}

fn repo_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]*/[A-Za-z0-9][A-Za-z0-9._-]*$").unwrap()
    })
}

pub fn validate_github_repo(s: &str) -> Result<&str, BuildError> {
    if s.len() > MAX_REPO_LEN {
        return Err(BuildError::InvalidInput {
            field: "github_repo",
            reason: "too long".into(),
        });
    }
    if !repo_re().is_match(s) {
        return Err(BuildError::InvalidInput {
            field: "github_repo",
            reason: "must match owner/repo".into(),
        });
    }
    Ok(s)
}

fn ref_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._/-]*$").unwrap())
}

pub fn validate_git_ref(s: &str) -> Result<&str, BuildError> {
    if s.len() > MAX_REF_LEN {
        return Err(BuildError::InvalidInput {
            field: "git_ref",
            reason: "too long".into(),
        });
    }
    if !ref_re().is_match(s) {
        return Err(BuildError::InvalidInput {
            field: "git_ref",
            reason: "disallowed characters".into(),
        });
    }
    if s.contains("..") || s.ends_with('/') || s.ends_with(".lock") {
        return Err(BuildError::InvalidInput {
            field: "git_ref",
            reason: "reserved shape".into(),
        });
    }
    Ok(s)
}

/// Caller embeds the result inside `RUN sh -c '<result>'`. Newlines/NUL rejected.
pub fn shell_single_quote(cmd: &str, field: &'static str) -> Result<String, BuildError> {
    if cmd.len() > MAX_CMD_LEN {
        return Err(BuildError::InvalidInput {
            field,
            reason: "too long".into(),
        });
    }
    if cmd.contains('\0') {
        return Err(BuildError::InvalidInput {
            field,
            reason: "NUL byte".into(),
        });
    }
    if cmd.contains('\n') || cmd.contains('\r') {
        return Err(BuildError::InvalidInput {
            field,
            reason: "newline".into(),
        });
    }
    Ok(cmd.replace('\'', r"'\''"))
}

/// Caller drops the result into a Dockerfile `CMD ["sh","-c",<result>]` array.
pub fn cmd_to_json_token(cmd: &str, field: &'static str) -> Result<String, BuildError> {
    if cmd.len() > MAX_CMD_LEN {
        return Err(BuildError::InvalidInput {
            field,
            reason: "too long".into(),
        });
    }
    if cmd.contains('\0') {
        return Err(BuildError::InvalidInput {
            field,
            reason: "NUL byte".into(),
        });
    }
    serde_json::to_string(cmd).map_err(|e| BuildError::InvalidInput {
        field,
        reason: format!("json encode: {e}"),
    })
}

fn hostname_label_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$").unwrap())
}

pub fn validate_hostname(s: &str) -> Result<&str, BuildError> {
    if s.is_empty() || s.len() > 253 {
        return Err(BuildError::InvalidInput {
            field: "hostname",
            reason: "length must be 1..=253".into(),
        });
    }
    if s.chars().any(|c| c.is_ascii_uppercase()) {
        return Err(BuildError::InvalidInput {
            field: "hostname",
            reason: "must be lowercase".into(),
        });
    }
    let trimmed = s.strip_suffix('.').unwrap_or(s);
    if trimmed.is_empty() {
        return Err(BuildError::InvalidInput {
            field: "hostname",
            reason: "empty after trailing dot".into(),
        });
    }
    let re = hostname_label_re();
    for label in trimmed.split('.') {
        if !re.is_match(label) {
            return Err(BuildError::InvalidInput {
                field: "hostname",
                reason: format!("invalid label: {label:?}"),
            });
        }
    }
    Ok(s)
}

pub fn validate_sha_hex(s: &str) -> Result<&str, BuildError> {
    if s.is_empty() || s.len() > 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(BuildError::InvalidInput {
            field: "git_sha",
            reason: "not a hex sha".into(),
        });
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jdk_happy() {
        assert_eq!(parse_jdk_major("21").unwrap(), 21);
        assert_eq!(parse_jdk_major("  8 ").unwrap(), 8);
        assert_eq!(parse_jdk_major("17").unwrap(), 17);
    }

    #[test]
    fn jdk_rejects_injection() {
        assert!(parse_jdk_major("21 && rm").is_err());
        assert!(parse_jdk_major("21\nRUN evil").is_err());
        assert!(parse_jdk_major("21 AS x").is_err());
        assert!(parse_jdk_major("0").is_err());
        assert!(parse_jdk_major("100").is_err()); // out of range
        assert!(parse_jdk_major("21.0.2").is_err()); // expect major only
    }

    #[test]
    fn node_engines() {
        assert_eq!(parse_node_major_from_engines("20"), Some(20));
        assert_eq!(parse_node_major_from_engines(">=20"), Some(20));
        assert_eq!(parse_node_major_from_engines("^20.10"), Some(20));
        assert_eq!(parse_node_major_from_engines("20.x"), Some(20));
        assert_eq!(parse_node_major_from_engines("v22.5.1"), Some(22));
        assert!(parse_node_major_from_engines("latest").is_none());
    }

    #[test]
    fn python_minor() {
        assert_eq!(parse_python_minor("3.12").unwrap(), (3, 12));
        assert_eq!(parse_python_minor("3.12.4").unwrap(), (3, 12));
        assert!(parse_python_minor("2.7").is_err());
        assert!(parse_python_minor("3.99").is_err());
    }

    #[test]
    fn go_minor() {
        assert_eq!(parse_go_minor("1.22").unwrap(), (1, 22));
        assert_eq!(parse_go_minor("1.22.3").unwrap(), (1, 22));
        assert!(parse_go_minor("2.0").is_err());
        assert!(parse_go_minor("1.5").is_err());
    }

    #[test]
    fn repo_validation() {
        assert!(validate_github_repo("jbj338033/helloworld").is_ok());
        assert!(validate_github_repo("a/b").is_ok());
        assert!(validate_github_repo("../etc/passwd").is_err());
        assert!(validate_github_repo("jbj338033/hello; rm -rf").is_err());
        assert!(validate_github_repo("only_one_part").is_err());
        assert!(validate_github_repo("a//b").is_err());
    }

    #[test]
    fn ref_validation() {
        assert!(validate_git_ref("main").is_ok());
        assert!(validate_git_ref("feature/x").is_ok());
        assert!(validate_git_ref("release-1.2.3").is_ok());
        assert!(validate_git_ref("-flag").is_err());
        assert!(validate_git_ref("a..b").is_err());
        assert!(validate_git_ref("a.lock").is_err());
        assert!(validate_git_ref("with space").is_err());
    }

    #[test]
    fn shell_quote_escapes() {
        assert_eq!(shell_single_quote("foo bar", "x").unwrap(), "foo bar");
        assert_eq!(shell_single_quote("it's ok", "x").unwrap(), r"it'\''s ok");
        assert!(shell_single_quote("multi\nline", "x").is_err());
    }

    #[test]
    fn json_token_escapes() {
        assert_eq!(cmd_to_json_token("plain", "x").unwrap(), "\"plain\"");
        assert_eq!(
            cmd_to_json_token(r#"java -jar "/app/app.jar""#, "x").unwrap(),
            r#""java -jar \"/app/app.jar\"""#
        );
    }

    #[test]
    fn sha_hex() {
        assert!(validate_sha_hex("deadbeef1234").is_ok());
        assert!(validate_sha_hex("deadbeef-rm").is_err());
        assert!(validate_sha_hex("").is_err());
    }

    #[test]
    fn hostname_happy() {
        assert!(validate_hostname("a.com").is_ok());
        assert!(validate_hostname("a").is_ok());
        assert!(validate_hostname("sub.example.co.kr").is_ok());
        assert!(validate_hostname("xn--p1ai").is_ok());
        assert!(validate_hostname("a.com.").is_ok());
    }

    #[test]
    fn hostname_rejects_bad() {
        assert!(validate_hostname("").is_err());
        assert!(validate_hostname(" foo").is_err());
        assert!(validate_hostname("foo ").is_err());
        assert!(validate_hostname("-bad").is_err());
        assert!(validate_hostname("bad-").is_err());
        assert!(validate_hostname("a..b").is_err());
        assert!(validate_hostname("UPPER.com").is_err());
        assert!(validate_hostname("foo\n.com").is_err());
        assert!(validate_hostname("evil`Host(`bad.com`)`").is_err());
        assert!(validate_hostname(&"a".repeat(64)).is_err());
        assert!(validate_hostname(&format!("{}.com", "a".repeat(250))).is_err());
    }
}
