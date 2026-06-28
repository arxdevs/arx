use crate::auth::SESSION_COOKIE;
use axum::extract::Request;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

fn is_mutating(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn has_session_cookie(headers: &HeaderMap) -> bool {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|raw| {
            raw.split(';').any(|pair| {
                pair.split_once('=')
                    .map(|(k, _)| k.trim() == SESSION_COOKIE)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn has_bearer(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|h| h.starts_with("Bearer ") || h.starts_with("bearer "))
        .unwrap_or(false)
}

fn origin_host(headers: &HeaderMap) -> Option<String> {
    let origin = headers.get(header::ORIGIN)?.to_str().ok()?;
    let without_scheme = origin.split("://").nth(1)?;
    Some(without_scheme.to_string())
}

fn same_origin(headers: &HeaderMap) -> bool {
    if let Some(sfs) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        return matches!(sfs, "same-origin" | "none");
    }

    match (origin_host(headers), headers.get(header::HOST)) {
        (Some(origin), Some(host)) => host.to_str().map(|h| h == origin).unwrap_or(false),
        (None, _) => true,
        _ => false,
    }
}

pub async fn guard(request: Request, next: Next) -> Response {
    let headers = request.headers();

    let needs_check =
        is_mutating(request.method()) && has_session_cookie(headers) && !has_bearer(headers);

    if needs_check && !same_origin(headers) {
        return (StatusCode::FORBIDDEN, "cross-site request blocked (csrf)").into_response();
    }

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn get_is_not_mutating() {
        assert!(!is_mutating(&Method::GET));
        assert!(is_mutating(&Method::POST));
        assert!(is_mutating(&Method::DELETE));
    }

    #[test]
    fn bearer_is_detected() {
        assert!(has_bearer(&headers(&[("authorization", "Bearer abc")])));
        assert!(has_bearer(&headers(&[("authorization", "bearer abc")])));
        assert!(!has_bearer(&headers(&[("authorization", "Basic abc")])));
        assert!(!has_bearer(&HeaderMap::new()));
    }

    #[test]
    fn session_cookie_is_detected() {
        assert!(has_session_cookie(&headers(&[(
            "cookie",
            "foo=1; arx_session=tok; bar=2"
        )])));
        assert!(!has_session_cookie(&headers(&[("cookie", "foo=1; bar=2")])));
    }

    #[test]
    fn sec_fetch_site_same_origin_passes() {
        assert!(same_origin(&headers(&[("sec-fetch-site", "same-origin")])));
        assert!(same_origin(&headers(&[("sec-fetch-site", "none")])));
        assert!(!same_origin(&headers(&[("sec-fetch-site", "cross-site")])));
        assert!(!same_origin(&headers(&[("sec-fetch-site", "same-site")])));
    }

    #[test]
    fn origin_host_match_passes_when_no_sec_fetch_site() {
        assert!(same_origin(&headers(&[
            ("origin", "https://arx.example.com"),
            ("host", "arx.example.com"),
        ])));
        assert!(!same_origin(&headers(&[
            ("origin", "https://evil.example.com"),
            ("host", "arx.example.com"),
        ])));
    }

    #[test]
    fn missing_origin_is_allowed() {
        assert!(same_origin(&headers(&[("host", "arx.example.com")])));
    }
}
