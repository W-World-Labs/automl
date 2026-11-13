//! Unified auth extractor: session cookie or API key → `AuthContext`.

use std::collections::HashSet;

use automl_core::scope;
use axum::{extract::FromRequestParts, http::request::Parts};
use axum_extra::extract::cookie::CookieJar;
use uuid::Uuid;

use crate::{
    auth::{
        keys::{self, VerifiedKey},
        sso::{self, SESSION_COOKIE},
    },
    error::ApiError,
    state::AppState,
};

/// Who is calling — either a browser session scholar or a scholar-delegated
/// API key. A key authenticates its owner, not a separately bound agent.
#[derive(Debug, Clone)]
pub enum Principal {
    Scholar {
        scholar_id: Uuid,
        w_id: String,
        display_name: String,
    },
    ApiKey {
        key_id: Uuid,
        owner_id: Uuid,
        name: String,
    },
}

impl Principal {
    pub fn owner_id(&self) -> Uuid {
        match self {
            Self::Scholar { scholar_id, .. } => *scholar_id,
            Self::ApiKey { owner_id, .. } => *owner_id,
        }
    }
}

/// Authenticated request context. Handlers take this as an extractor.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub principal: Principal,
    pub scopes: HashSet<String>,
}

impl AuthContext {
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.contains(scope::ADMIN) || self.scopes.contains(scope)
    }

    pub fn require_scope(&self, scope: &str) -> Result<(), ApiError> {
        if self.has_scope(scope) {
            Ok(())
        } else {
            Err(ApiError::forbidden(format!("missing scope: {scope}")))
        }
    }

    pub fn is_admin(&self) -> bool {
        self.scopes.contains(scope::ADMIN)
    }

    pub fn owner_id(&self) -> Uuid {
        self.principal.owner_id()
    }

    pub fn scopes_vec(&self) -> Vec<String> {
        let mut v: Vec<String> = self.scopes.iter().cloned().collect();
        v.sort();
        v
    }
}

impl FromRequestParts<AppState> for AuthContext {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(token) = bearer_token(parts) {
            return match keys::verify_key(&state.db, token).await? {
                Some(vk) => Ok(from_api_key(vk)),
                None => Err(ApiError::unauthorized("invalid or expired API key")),
            };
        }

        match session_auth(parts, state).await? {
            Some(ctx) => Ok(ctx),
            None => Err(ApiError::unauthorized(
                "authentication required (Bearer amk_… or W-ID session cookie)",
            )),
        }
    }
}

fn from_api_key(vk: VerifiedKey) -> AuthContext {
    AuthContext {
        principal: Principal::ApiKey {
            key_id: vk.key_id,
            owner_id: vk.owner_id,
            name: vk.name,
        },
        scopes: vk.scopes.into_iter().collect(),
    }
}

async fn session_auth(parts: &Parts, state: &AppState) -> Result<Option<AuthContext>, ApiError> {
    let Some(sso) = state.sso.as_ref() else {
        return Ok(None);
    };

    // CookieJar::from_headers does not consume the request.
    let jar = CookieJar::from_headers(&parts.headers);
    let Some(session) = w_id_axum::read_session(Some(sso), &jar, SESSION_COOKIE) else {
        return Ok(None);
    };

    let scholar_id = match sso::scholar_id_for_w_id(&state.db, &session.sub).await? {
        Some(id) => id,
        None => {
            // Session valid but scholar row missing (mirror lag) — provision now.
            sso::ensure_scholar(&state.db, &session.sub, &session.login).await?
        }
    };

    let mut scopes: HashSet<String> = scope::SCHOLAR_DEFAULT
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    if session.is_admin {
        scopes.insert(scope::ADMIN.to_string());
    }

    Ok(Some(AuthContext {
        principal: Principal::Scholar {
            scholar_id,
            w_id: session.sub,
            display_name: session.login,
        },
        scopes,
    }))
}

fn bearer_token(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_scope_implies_all() {
        let ctx = AuthContext {
            principal: Principal::Scholar {
                scholar_id: Uuid::nil(),
                w_id: "x".into(),
                display_name: "x".into(),
            },
            scopes: HashSet::from([scope::ADMIN.to_string()]),
        };
        assert!(ctx.has_scope(scope::PAPERS_WRITE));
        assert!(ctx.require_scope(scope::KEYS_MANAGE).is_ok());
    }

    #[test]
    fn missing_scope_is_forbidden() {
        let ctx = AuthContext {
            principal: Principal::ApiKey {
                key_id: Uuid::nil(),
                owner_id: Uuid::nil(),
                name: "cli".into(),
            },
            scopes: HashSet::from([scope::PAPERS_READ.to_string()]),
        };
        let err = ctx.require_scope(scope::PAPERS_WRITE).unwrap_err();
        assert_eq!(err.code, automl_core::ErrorCode::Forbidden);
    }
}
