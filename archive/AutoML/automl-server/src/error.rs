// ============================================================
// Unified error handling
//
// Migrated onto the fleet-shared `w_web_kit::error::WebError` (features
// `error` + `error-sqlx` + `error-anyhow`). The local hand-rolled
// `struct ApiError {status,message,hint}` was byte-identical to WebError's
// Plain envelope, so this module is now a thin re-export: `ApiError` aliases
// `WebError`, `ApiResult` aliases `WebResult`. All call sites
// (`ApiError::bad_request/not_found/internal/...` + `.with_hint(..)` +
// `?`-conversion from `sqlx::Error`/`anyhow::Error`) keep working unchanged.
//
// Wire parity (asserted by the golden tests below, green before AND after the
// swap):
//   * `{"error": msg}` and `{"error": msg, "hint": hint}` envelopes,
//   * status codes for bad_request/unauthorized/forbidden/service_unavailable/
//     not_found/internal,
//   * From<sqlx::Error>  -> 500 `{"error":"Database error"}`,
//   * From<anyhow::Error> -> 500 `{"error":"Internal server error"}`.
//
// DEVIATION (non-wire): the error-detail log on the two `From` impls now routes
// through w-web-kit's logging shim (`eprintln!("[w-web-kit] 500 Database error:
// {e}")`) instead of the old `tracing::error!`. Log routing only — the HTTP
// response (status + body + headers) is byte-identical.
// ============================================================

pub use w_web_kit::error::{WebError as ApiError, WebResult as ApiResult};

#[cfg(test)]
mod golden {
    //! Wire-faithfulness golden test. Captures the EXACT status + raw JSON body
    //! bytes the automl error envelope emits. Authored against the original
    //! local `ApiError` (green), retained verbatim across the swap onto
    //! `w_web_kit::error::WebError` (the re-export alias `ApiError = WebError`),
    //! and still green — proving the migration is wire-invisible.
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;

    async fn raw(err: ApiError) -> (u16, String) {
        let resp = err.into_response();
        let status = resp.status().as_u16();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn bad_request_plain_body_is_byte_identical() {
        let (s, b) = raw(ApiError::bad_request("bad input")).await;
        assert_eq!(s, 400);
        assert_eq!(b, r#"{"error":"bad input"}"#);
    }

    #[tokio::test]
    async fn bad_request_with_hint_body_is_byte_identical() {
        let (s, b) = raw(ApiError::bad_request("bad input").with_hint("try smaller")).await;
        assert_eq!(s, 400);
        assert_eq!(b, r#"{"error":"bad input","hint":"try smaller"}"#);
    }

    #[tokio::test]
    async fn not_found_body_is_byte_identical() {
        let (s, b) = raw(ApiError::not_found("missing")).await;
        assert_eq!(s, 404);
        assert_eq!(b, r#"{"error":"missing"}"#);
    }

    #[tokio::test]
    async fn status_ctor_codes_unchanged() {
        assert_eq!(raw(ApiError::unauthorized("x")).await.0, 401);
        assert_eq!(raw(ApiError::forbidden("x")).await.0, 403);
        assert_eq!(raw(ApiError::service_unavailable("x")).await.0, 503);
        assert_eq!(raw(ApiError::internal("x")).await.0, 500);
    }

    #[tokio::test]
    async fn from_sqlx_error_is_generic_500_byte_identical() {
        let err: ApiError = sqlx::Error::RowNotFound.into();
        let (s, b) = raw(err).await;
        assert_eq!(s, 500);
        assert_eq!(b, r#"{"error":"Database error"}"#);
    }

    #[tokio::test]
    async fn from_anyhow_error_is_generic_500_byte_identical() {
        let err: ApiError = anyhow::anyhow!("some internal detail").into();
        let (s, b) = raw(err).await;
        assert_eq!(s, 500);
        assert_eq!(b, r#"{"error":"Internal server error"}"#);
    }
}
