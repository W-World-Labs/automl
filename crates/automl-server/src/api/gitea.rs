//! Inbound webhook from the dedicated Gitea instance.
//!
//! Signature IS the authentication: the raw body is verified against
//! `X-Gitea-Signature` (HMAC-SHA256, constant-time) BEFORE any parsing.
//! The route carries no session/key auth but sits behind the write rate bucket.

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};

use crate::{
    domain::gitea::{self, EventOutcome},
    error::{ApiError, ApiResult},
    state::AppState,
};

#[utoipa::path(
    post, path = "/gitea/events", tag = "system",
    responses(
        (status = 204, description = "Event processed or ignored"),
        (status = 401, description = "Missing/invalid signature"),
        (status = 501, description = "Code-hosting integration not configured"),
    )
)]
pub async fn gitea_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    let Some(client) = state.gitea.as_ref() else {
        return Err(ApiError::not_implemented(
            "code-hosting integration is not configured on this deployment",
        ));
    };

    // Verify before parse — unauthenticated bytes never reach serde.
    let signature = headers
        .get("x-gitea-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("missing X-Gitea-Signature"))?;
    if !client.verify_signature(&body, signature) {
        return Err(ApiError::unauthorized("invalid webhook signature"));
    }

    let event_type = headers
        .get("x-gitea-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let payload: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::invalid("webhook payload is not valid JSON"))?;

    match gitea::apply_event(&state.db, event_type, &payload).await? {
        EventOutcome::Applied { paper_id } => {
            tracing::info!(%paper_id, "gitea push applied");
        }
        EventOutcome::Ignored => {}
    }
    Ok(StatusCode::NO_CONTENT)
}
