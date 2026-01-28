use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Missing authentication headers: X-Auth-Address, X-Auth-Signature, and X-Auth-Timestamp are required")]
    ApiHeadersMissing,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            ApiError::ApiHeadersMissing => StatusCode::BAD_REQUEST,
        };

        let body = Json(ErrorResponse {
            error: self.to_string(),
        });

        (status, body).into_response()
    }
}
