//! The one error shape every REST endpoint returns (design section 6):
//! `{ "error": { "code": "QUIZ_NOT_FOUND", "message": "..." } }`.
//! The client translates the code; the message is for developers. Internal
//! errors are logged and returned as `INTERNAL` without details (SEC-26).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use myquiz_protocol::{ApiError, ApiErrorBody};

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

pub type ApiResult<T> = Result<T, AppError>;

impl AppError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        AppError {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn unauthenticated() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "UNAUTHENTICATED", "sign in first")
    }

    pub fn forbidden(code: &'static str) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, "not allowed")
    }

    #[allow(dead_code)] // quiz endpoints, next PR
    pub fn not_found(code: &'static str) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, "not found")
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        let e: anyhow::Error = e.into();
        tracing::error!(error = ?e, "internal error");
        AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            "internal error",
        )
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = ApiError {
            error: ApiErrorBody {
                code: self.code.to_string(),
                message: self.message,
            },
        };
        (self.status, Json(body)).into_response()
    }
}
