use automl_core::ErrorCode;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// The one error type every handler returns. Serializes as
/// `{"error": {"code": "...", "message": "..."}}` — `code` is the stable
/// machine contract, `message` is for humans and may change freely.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: ErrorCode,
    pub message: String,
    pub retry_after: Option<u64>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retry_after: None,
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, ErrorCode::InvalidInput, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, ErrorCode::NotFound, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, ErrorCode::Forbidden, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, ErrorCode::Conflict, message)
    }

    pub fn rate_limited(wait_secs: u64) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: ErrorCode::RateLimited,
            message: format!("rate limit exceeded; retry after {wait_secs}s"),
            retry_after: Some(wait_secs),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::Unavailable,
            message,
        )
    }

    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::NOT_IMPLEMENTED,
            ErrorCode::NotImplemented,
            message,
        )
    }

    pub fn signature_invalid(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::SignatureInvalid,
            message,
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({"error": {"code": self.code.as_str(), "message": self.message}});
        let mut resp = (self.status, Json(body)).into_response();
        if let Some(secs) = self.retry_after {
            if let Ok(val) = http::HeaderValue::from_str(&secs.to_string()) {
                resp.headers_mut().insert(http::header::RETRY_AFTER, val);
            }
        }
        resp
    }
}

// Internal causes are logged with full detail but never leave the process —
// clients get the generic envelope only.
impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        if matches!(e, sqlx::Error::RowNotFound) {
            return Self::not_found("resource not found");
        }
        tracing::error!(error = %e, "database error");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            "database error",
        )
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!(error = %e, "internal error");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            "internal server error",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn envelope_shape_is_stable() {
        let resp = ApiError::not_found("paper not found").into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], "NOT_FOUND");
        assert_eq!(v["error"]["message"], "paper not found");
    }

    #[tokio::test]
    async fn rate_limited_sets_retry_after() {
        let resp = ApiError::rate_limited(3).into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(resp.headers().get(http::header::RETRY_AFTER).unwrap(), "3");
        let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], "RATE_LIMITED");
    }
}
