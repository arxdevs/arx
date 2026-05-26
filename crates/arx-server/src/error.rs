use arx_core::Error as CoreError;
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub &'static str, pub String);

impl ApiError {
    pub fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, "not_found", "not found".into())
    }
    pub fn unauthorized() -> Self {
        Self(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "unauthorized".into(),
        )
    }
    pub fn unauthorized_with(msg: impl Into<String>) -> Self {
        Self(StatusCode::UNAUTHORIZED, "unauthorized", msg.into())
    }
    pub fn forbidden() -> Self {
        Self(StatusCode::FORBIDDEN, "forbidden", "forbidden".into())
    }
    pub fn already_exists(msg: impl Into<String>) -> Self {
        Self(StatusCode::CONFLICT, "already_exists", msg.into())
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, "bad_request", msg.into())
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, "internal", msg.into())
    }
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::NotFound => Self::not_found(),
            CoreError::AlreadyExists => Self::already_exists("already exists"),
            CoreError::Unauthorized => Self::unauthorized(),
            CoreError::Forbidden => Self::forbidden(),
            CoreError::InvalidInput(m) => Self::bad_request(m),
            CoreError::Conflict(m) => Self::already_exists(m),
            other => Self::internal(other.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            code: self.1,
            message: self.2,
        };
        (self.0, Json(body)).into_response()
    }
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;
