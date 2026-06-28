use crate::error::ApiError;
use crate::state::AppState;
use arx_db::queries::auth::{AuthedUser, authenticate};
use async_trait::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::request::Parts;

pub const SESSION_COOKIE: &str = "arx_session";

pub struct Auth(pub AuthedUser);

fn bearer_token(parts: &Parts) -> Option<String> {
    let header = parts.headers.get(AUTHORIZATION)?.to_str().ok()?;
    header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .map(str::to_string)
}

fn cookie_token(parts: &Parts, name: &str) -> Option<String> {
    let header = parts.headers.get(COOKIE)?.to_str().ok()?;
    header.split(';').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k.trim() == name).then(|| v.trim().to_string())
    })
}

#[async_trait]
impl<S> FromRequestParts<S> for Auth
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts)
            .or_else(|| cookie_token(parts, SESSION_COOKIE))
            .ok_or_else(ApiError::unauthorized)?;

        let app: AppState = AppState::from_ref(state);
        let user = authenticate(&app.db, &token).await?;
        Ok(Auth(user))
    }
}
