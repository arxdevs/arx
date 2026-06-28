use crate::auth::SESSION_COOKIE;
use crate::state::AppState;
use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web-dist/"]
struct Assets;

const NOT_BUILT_HTML: &str = "<!doctype html><html><head><meta charset=\"utf-8\">\
<title>arx</title></head><body style=\"font-family:system-ui;text-align:center;margin-top:4em\">\
<h1>arx</h1><p>web UI is not built into this binary.</p>\
<p>Run <code>npm run build</code> in <code>web/</code> or use a release image.</p>\
</body></html>";

pub async fn spa_fallback(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if !path.is_empty() {
        if let Some(asset) = Assets::get(path) {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            return (
                [(header::CONTENT_TYPE, mime.as_ref())],
                asset.data.into_owned(),
            )
                .into_response();
        }
    }

    match Assets::get("index.html") {
        Some(index) => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            index.data.into_owned(),
        )
            .into_response(),
        None => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            NOT_BUILT_HTML,
        )
            .into_response(),
    }
}

/// Whether session cookies should carry the `Secure` attribute. We decide from
/// arx's own configuration rather than a spoofable `X-Forwarded-Proto` header:
/// any admin domain means Traefik is fronting us over HTTPS. Absence of an admin
/// domain only happens in pure-local development on the loopback bind, where
/// `Secure` would otherwise block the cookie over plain HTTP. Fail closed.
pub async fn cookie_is_secure(app: &AppState) -> bool {
    match arx_db::queries::settings::get(&app.db).await {
        Ok(s) => s.admin_domain.is_some(),
        Err(_) => true,
    }
}

pub fn session_cookie(token: &str, max_age_secs: i64, secure: bool) -> String {
    set_cookie(SESSION_COOKIE, token, max_age_secs, secure)
}

pub fn clear_cookie(name: &str, secure: bool) -> String {
    set_cookie(name, "", 0, secure)
}

pub fn set_cookie(name: &str, value: &str, max_age_secs: i64, secure: bool) -> String {
    let mut cookie =
        format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

pub fn redirect_to_root(cookies: Vec<String>) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, "/");
    for c in cookies {
        builder = builder.header(header::SET_COOKIE, c);
    }
    builder
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
