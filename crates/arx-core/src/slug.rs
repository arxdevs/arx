use crate::{Error, Result};

const MAX_LEN: usize = 63;

pub fn validate(field: &'static str, s: &str) -> Result<()> {
    if s.is_empty() {
        return Err(Error::InvalidInput(format!("{field} must not be empty")));
    }
    if s.len() > MAX_LEN {
        return Err(Error::InvalidInput(format!(
            "{field} too long (max {MAX_LEN})"
        )));
    }
    let bytes = s.as_bytes();
    let starts_ok = bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit();
    let ends_ok =
        bytes[bytes.len() - 1].is_ascii_lowercase() || bytes[bytes.len() - 1].is_ascii_digit();
    if !starts_ok || !ends_ok {
        return Err(Error::InvalidInput(format!(
            "{field} must start and end with [a-z0-9]"
        )));
    }
    let body_ok = s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !body_ok {
        return Err(Error::InvalidInput(format!(
            "{field} may only contain [a-z0-9_-]"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple() {
        assert!(validate("slug", "prod").is_ok());
        assert!(validate("slug", "pg").is_ok());
        assert!(validate("slug", "my-svc_1").is_ok());
        assert!(validate("slug", "a").is_ok());
        assert!(validate("slug", "0").is_ok());
    }

    #[test]
    fn rejects_empty() {
        assert!(validate("slug", "").is_err());
    }

    #[test]
    fn rejects_uppercase() {
        assert!(validate("slug", "Prod").is_err());
    }

    #[test]
    fn rejects_disallowed_chars() {
        assert!(validate("slug", "my/svc").is_err());
        assert!(validate("slug", "svc@1").is_err());
        assert!(validate("slug", "svc.1").is_err());
        assert!(validate("slug", "svc 1").is_err());
    }

    #[test]
    fn rejects_edge_hyphen_underscore() {
        assert!(validate("slug", "-svc").is_err());
        assert!(validate("slug", "svc-").is_err());
        assert!(validate("slug", "_svc").is_err());
        assert!(validate("slug", "svc_").is_err());
    }

    #[test]
    fn rejects_too_long() {
        let s = "a".repeat(64);
        assert!(validate("slug", &s).is_err());
        let ok = "a".repeat(63);
        assert!(validate("slug", &ok).is_ok());
    }
}
