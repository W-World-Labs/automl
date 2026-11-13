//! Authentication & authorization (iteration 2).
//!
//! - W-ID SSO via `w-id-axum` (`/auth/{login,callback,me,logout}`)
//! - API key lifecycle (`amk_…`, sha256 + ct_eq)
//! - Scope-based `AuthContext` (replaces v1 `trust_level`)
//! - `tower_governor` rate limits
//! - Dual-gated dev-admin backdoor

pub mod cli_auth;
pub mod context;
pub mod dev_admin;
pub mod keys;
pub mod project_acl;
pub mod rate_limit;
pub mod sso;

pub use context::{AuthContext, Principal};
