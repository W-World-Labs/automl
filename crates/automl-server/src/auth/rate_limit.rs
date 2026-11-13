//! Rate limiting via `tower_governor`.
//!
//! Key priority: API key id prefix > session cookie fingerprint > client IP.
//! Write methods get a stricter secondary layer. Quotas come from config.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Method, Request, StatusCode},
    response::{IntoResponse, Response},
};
use governor::middleware::NoOpMiddleware;
use tower_governor::{
    governor::GovernorConfigBuilder, key_extractor::KeyExtractor, GovernorError, GovernorLayer,
};

use crate::{
    auth::{keys, sso::SESSION_COOKIE},
    config::RateLimitConfig,
    error::ApiError,
};

/// Composite key string used by the governor bucket.
#[derive(Clone, Debug)]
pub struct AuthKeyExtractor;

impl KeyExtractor for AuthKeyExtractor {
    type Key = String;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        if let Some(token) = bearer_from_req(req) {
            if let Some(prefix) = keys::key_id_prefix(token) {
                return Ok(format!("key:{prefix}"));
            }
            // Non-amk bearer still buckets by a short hash of the token material.
            let digest = w_crypto::sha256(token.as_bytes());
            return Ok(format!(
                "bearer:{:02x}{:02x}{:02x}{:02x}",
                digest[0], digest[1], digest[2], digest[3]
            ));
        }

        if let Some(cookie) = session_cookie(req) {
            let digest = w_crypto::sha256(cookie.as_bytes());
            return Ok(format!(
                "session:{:02x}{:02x}{:02x}{:02x}",
                digest[0], digest[1], digest[2], digest[3]
            ));
        }

        let ip = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().to_string())
            .or_else(|| {
                req.headers()
                    .get("x-forwarded-for")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.split(',').next())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "unknown".into());
        Ok(format!("ip:{ip}"))
    }
}

fn bearer_from_req<T>(req: &Request<T>) -> Option<&str> {
    req.headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

fn session_cookie<T>(req: &Request<T>) -> Option<String> {
    let header = req.headers().get(http::header::COOKIE)?.to_str().ok()?;
    for part in header.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix(&format!("{SESSION_COOKIE}=")) {
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

pub type AuthGovernorLayer = GovernorLayer<AuthKeyExtractor, NoOpMiddleware, Body>;

pub fn governor_layer(cfg: &RateLimitConfig) -> AuthGovernorLayer {
    // `key_extractor` takes &mut self and returns a new builder by value —
    // do not chain it into a discarded temporary.
    let mut builder = GovernorConfigBuilder::default()
        .const_per_second(cfg.rps)
        .const_burst_size(cfg.burst);
    let mut builder = builder.key_extractor(AuthKeyExtractor);
    let conf = builder.finish().expect("valid governor config");
    GovernorLayer::new(Arc::new(conf)).error_handler(rate_limit_response)
}

/// Strictest bucket, applied only to the anonymous feedback route.
pub fn feedback_governor_layer(cfg: &RateLimitConfig) -> AuthGovernorLayer {
    let mut builder = GovernorConfigBuilder::default()
        .const_per_second(cfg.feedback_rps)
        .const_burst_size(cfg.feedback_burst);
    let mut builder = builder.key_extractor(AuthKeyExtractor);
    let conf = builder.finish().expect("valid feedback governor config");
    GovernorLayer::new(Arc::new(conf)).error_handler(rate_limit_response)
}

pub fn write_governor_layer(cfg: &RateLimitConfig) -> AuthGovernorLayer {
    let mut builder = GovernorConfigBuilder::default()
        .const_per_second(cfg.write_rps)
        .const_burst_size(cfg.write_burst);
    builder.methods(vec![
        Method::POST,
        Method::PUT,
        Method::PATCH,
        Method::DELETE,
    ]);
    let mut builder = builder.key_extractor(AuthKeyExtractor);
    let conf = builder.finish().expect("valid write governor config");
    GovernorLayer::new(Arc::new(conf)).error_handler(rate_limit_response)
}

fn rate_limit_response(err: GovernorError) -> Response<Body> {
    match err {
        GovernorError::TooManyRequests { wait_time, .. } => {
            ApiError::rate_limited(wait_time).into_response()
        }
        GovernorError::UnableToExtractKey => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({
                "error": {"code": "INTERNAL", "message": "rate limit key extraction failed"}
            })),
        )
            .into_response(),
        GovernorError::Other { code, msg, .. } => {
            let message = msg.unwrap_or_else(|| "rate limit error".into());
            (
                code,
                axum::Json(serde_json::json!({
                    "error": {"code": "INTERNAL", "message": message}
                })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[test]
    fn prefers_api_key_prefix() {
        let req = Request::builder()
            .header("authorization", "Bearer amk_deadbeef_secretvalue")
            .body(())
            .unwrap();
        let key = AuthKeyExtractor.extract(&req).unwrap();
        assert_eq!(key, "key:deadbeef");
    }

    #[test]
    fn falls_back_to_session_cookie() {
        let req = Request::builder()
            .header("cookie", "automl_session=tok123; other=1")
            .body(())
            .unwrap();
        let key = AuthKeyExtractor.extract(&req).unwrap();
        assert!(key.starts_with("session:"));
    }

    #[test]
    fn falls_back_to_unknown_ip() {
        let req = Request::builder().body(()).unwrap();
        let key = AuthKeyExtractor.extract(&req).unwrap();
        assert_eq!(key, "ip:unknown");
    }
}
