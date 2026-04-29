use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("gateway error: {0}")]
    Gateway(#[from] GatewayError),

    #[error("authentication error: {0}")]
    Auth(#[from] AuthError),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("TOU error: {0}")]
    Tou(#[from] TouError),

    #[error("API error: {0}")]
    Api(#[from] ApiError),
}

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("malformed response: {0}")]
    MalformedResponse(String),
    #[error("gateway unreachable: {0}")]
    Unreachable(String),
    #[error("unauthorized (401) — session token may have expired")]
    Unauthorized,
}

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum AuthError {
    #[error("invalid token: {0}")]
    InvalidToken(String),
    #[error("token expired")]
    TokenExpired,
    #[error("token missing")]
    TokenMissing,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

#[derive(Debug, Error)]
pub enum TouError {
    #[error("upstream unavailable: {0}")]
    UpstreamUnavailable(String),
    #[error("no rate schedule available")]
    NoSchedule,
    #[error("parse error: {0}")]
    ParseError(String),
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("invalid parameter: {0}")]
    InvalidParam(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("insufficient data: {0}")]
    InsufficientData(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            AppError::Api(ApiError::InvalidParam(m)) => {
                (StatusCode::BAD_REQUEST, "invalid_param", m.clone())
            }
            AppError::Api(ApiError::NotFound(m)) => (StatusCode::NOT_FOUND, "not_found", m.clone()),
            AppError::Api(ApiError::InsufficientData(m)) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "insufficient_data",
                m.clone(),
            ),
            AppError::Tou(TouError::NoSchedule) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "no_tou_schedule",
                "no TOU rate schedule available; run POST /api/tou/refresh first".into(),
            ),
            AppError::Tou(TouError::UpstreamUnavailable(m)) => {
                (StatusCode::BAD_GATEWAY, "upstream_unavailable", m.clone())
            }
            _ => {
                tracing::error!(error = %self, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    self.to_string(),
                )
            }
        };
        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}
