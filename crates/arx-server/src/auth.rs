use crate::error::ApiError;
use crate::state::AppState;
use arx_db::queries::auth::{AuthedUser, authenticate};
use async_trait::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;

pub struct Auth(pub AuthedUser);

#[async_trait]
impl<S> FromRequestParts<S> for Auth
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let token = {
            let header = parts
                .headers
                .get(AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .ok_or(ApiError::unauthorized())?;
            header
                .strip_prefix("Bearer ")
                .or_else(|| header.strip_prefix("bearer "))
                .ok_or(ApiError::unauthorized())?
                .to_string()
        };

        let app: AppState = AppState::from_ref(state);
        let user = authenticate(&app.db, &token).await?;
        Ok(Auth(user))
    }
}
