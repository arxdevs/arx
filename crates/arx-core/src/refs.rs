use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub service_slug: String,
    pub variable_key: String,

    pub span: (usize, usize),
}

fn re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\$\{\{\s*([A-Za-z0-9_-]+)\.([A-Za-z_][A-Za-z0-9_]*)\s*\}\}").unwrap()
    })
}

pub fn find_references(input: &str) -> Vec<Reference> {
    re().captures_iter(input)
        .map(|c| {
            let whole = c.get(0).unwrap();
            Reference {
                service_slug: c.get(1).unwrap().as_str().to_string(),
                variable_key: c.get(2).unwrap().as_str().to_string(),
                span: (whole.start(), whole.end()),
            }
        })
        .collect()
}

pub fn substitute(
    input: &str,
    mut lookup: impl FnMut(&str, &str) -> Result<String, ResolveError>,
) -> Result<String, ResolveError> {
    let mut out = String::with_capacity(input.len());
    let mut last = 0;
    for r in find_references(input) {
        let (start, end) = r.span;
        out.push_str(&input[last..start]);
        let v = lookup(&r.service_slug, &r.variable_key)?;
        out.push_str(&v);
        last = end;
    }
    out.push_str(&input[last..]);
    Ok(out)
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ResolveError {
    #[error("variable references service `{service}` but no such service in this environment")]
    ServiceNotFound { service: String },

    #[error("variable references {service}.{var} but that variable doesn't exist")]
    VariableNotFound { service: String, var: String },

    #[error("reference cycle: {path}")]
    Cycle { path: String },

    #[error("reference depth exceeded ({max}) — likely a cycle")]
    DepthExceeded { max: u32 },

    #[error("failed to look up referenced variable for service `{service}`: {detail}")]
    LookupFailed { service: String, detail: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        let refs = find_references("hello ${{Postgres.DATABASE_URL}} world");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].service_slug, "Postgres");
        assert_eq!(refs[0].variable_key, "DATABASE_URL");
    }

    #[test]
    fn parse_whitespace() {
        let refs = find_references("${{ Svc.X }}");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].service_slug, "Svc");
        assert_eq!(refs[0].variable_key, "X");
    }

    #[test]
    fn parse_multiple() {
        let refs = find_references("${{a.B}}-${{c.D}}");
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn parse_none() {
        assert!(find_references("plain string").is_empty());
        assert!(find_references("${PG_URL}").is_empty());
        assert!(find_references("{{X.Y}}").is_empty());
    }

    #[test]
    fn substitute_simple() {
        let out = substitute("URL=${{Pg.URL}}", |_, _| Ok("postgres://".into())).unwrap();
        assert_eq!(out, "URL=postgres://");
    }

    #[test]
    fn substitute_chain() {
        let out = substitute("${{a.x}}-${{b.y}}", |s, k| Ok(format!("{s}_{k}"))).unwrap();
        assert_eq!(out, "a_x-b_y");
    }

    #[test]
    fn substitute_error_propagates() {
        let r = substitute("${{Postgres.URL}}", |_, _| {
            Err(ResolveError::VariableNotFound {
                service: "Postgres".into(),
                var: "URL".into(),
            })
        });
        assert!(r.is_err());
    }
}
