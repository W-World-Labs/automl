//! Domain layer — pure rules + persistence for papers core.
//!
//! HTTP handlers in `crate::api` depend on this module; this module never
//! depends on axum. Keeps state machines, evidence validation, and
//! verified-by-N aggregation free of transport concerns.

pub mod agents;
pub mod assets;
pub mod compute;
pub mod cursor;
pub mod events;
pub mod feedback;
pub mod forum;
pub mod gitea;
pub mod papers;
pub mod project_assets;
pub mod project_bindings;
pub mod project_papers;
pub mod projects;
pub mod reproduce;
pub mod reviews;
pub mod scholars;
pub mod templates;
