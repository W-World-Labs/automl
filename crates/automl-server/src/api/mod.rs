//! API surface assembly.
//!
//! Every route is registered through `utoipa_axum::routes!` so the OpenAPI
//! document is generated from the code it describes — there is no parallel
//! hand-maintained route list (v1's openapi.rs drift class is structurally
//! impossible here).

pub mod agents;
pub mod assets;
pub mod cli;
pub mod compute;
pub mod feedback;
pub mod forum;
pub mod gitea;
pub mod health;
pub mod keys;
pub mod papers;
pub mod projects;
pub mod reproduce;
pub mod reviews;
pub mod scholars;
pub mod templates;
pub mod webhooks;

use utoipa::{
    openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme},
    Modify, OpenApi,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    auth::{self, sso},
    config::Config,
    state::AppState,
};

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer_api_key",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("amk_*")
                    .description(Some(
                        "API key issued by POST /api/v1/keys. Header: Authorization: Bearer amk_…",
                    ))
                    .build(),
            ),
        );
        components.add_security_scheme(
            "session_cookie",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::with_description(
                sso::SESSION_COOKIE,
                "W-ID session cookie set by /api/v1/auth/callback",
            ))),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Auto.ml API",
        description = "Automated research platform: papers, reviews, reproduction runs, scholars, agents.",
        version = env!("CARGO_PKG_VERSION"),
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "system", description = "Health, metrics, platform introspection"),
        (name = "auth", description = "SSO, API keys, scopes"),
        (name = "papers", description = "Paper pointers and review status"),
        (name = "reviews", description = "Peer reviews"),
        (name = "reproduce", description = "Client-side reproduce evidence chain"),
        (name = "assets", description = "Presigned asset upload"),
        (name = "identity", description = "Scholars, agents, device keys"),
        (name = "compute", description = "Client compute profiles (provenance only)"),
        (name = "templates", description = "Templates and skills manifests"),
        (name = "community", description = "Forum, feedback, notifications"),
        (name = "webhooks", description = "Outbound webhook subscriptions and deliveries"),
        (name = "projects", description = "Research Hub projects, members, audit (P0)"),
    )
)]
struct ApiDoc;

/// Build the `/api/v1` router (including SSO + optional dev-admin) and OpenAPI doc.
pub fn v1(cfg: &Config) -> (axum::Router<AppState>, utoipa::openapi::OpenApi) {
    // Group by path so MethodRouter merges (utoipa_axum panics on duplicate
    // method handlers when paths collide across separate routes! invocations).
    let mut router = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health::health))
        .routes(routes!(cli::cli_install))
        .routes(routes!(cli::cli_auth_start))
        .routes(routes!(cli::cli_auth_token))
        .routes(routes!(keys::create_key, keys::list_keys, keys::revoke_key))
        .routes(routes!(papers::create_paper, papers::list_papers))
        .routes(routes!(papers::get_paper, papers::patch_paper))
        .routes(routes!(reviews::create_review, reviews::list_reviews))
        .routes(routes!(reproduce::register_run, reproduce::list_runs))
        .routes(routes!(reproduce::get_run, reproduce::submit_evidence))
        .routes(routes!(reproduce::paper_reproduction))
        .routes(routes!(assets::create_asset))
        .routes(routes!(assets::commit_asset, assets::get_asset))
        .routes(routes!(scholars::list_scholars))
        .routes(routes!(scholars::get_scholar, scholars::patch_scholar))
        .routes(routes!(agents::create_agent, agents::list_agents))
        .routes(routes!(agents::register_device, agents::list_devices))
        .routes(routes!(agents::revoke_device))
        .routes(routes!(compute::create_profile, compute::list_profiles))
        .routes(routes!(
            templates::create_template,
            templates::list_templates
        ))
        .routes(routes!(
            templates::get_template,
            templates::patch_template,
            templates::delete_template
        ))
        .routes(routes!(templates::get_manifest))
        .routes(routes!(gitea::gitea_events))
        .routes(routes!(forum::list_categories))
        .routes(routes!(forum::create_thread, forum::list_threads))
        .routes(routes!(forum::get_thread, forum::delete_thread))
        .routes(routes!(forum::create_post, forum::list_posts))
        .routes(routes!(forum::delete_post))
        .routes(routes!(forum::notifications))
        .routes(routes!(feedback::list_feedback))
        .routes(routes!(feedback::patch_feedback))
        .routes(routes!(
            webhooks::create_subscription,
            webhooks::list_subscriptions
        ))
        .routes(routes!(
            webhooks::patch_subscription,
            webhooks::delete_subscription
        ))
        .routes(routes!(webhooks::list_deliveries))
        .routes(routes!(projects::create_project, projects::list_projects))
        .routes(routes!(projects::get_project, projects::patch_project))
        .routes(routes!(projects::add_member, projects::list_members))
        .routes(routes!(projects::remove_member))
        .routes(routes!(projects::list_audit))
        .routes(routes!(
            projects::add_project_paper,
            projects::list_project_papers
        ))
        .routes(routes!(
            projects::bind_daas,
            projects::get_daas_binding,
            projects::unbind_daas
        ))
        .routes(routes!(
            projects::add_project_asset,
            projects::list_project_assets
        ))
        .routes(routes!(
            projects::get_project_asset,
            projects::delete_project_asset
        ))
        .routes(routes!(projects::import_project_pdf))
        .routes(routes!(projects::search_project));

    if cfg.dev_admin_enabled() {
        router = router.routes(routes!(auth::dev_admin::dev_admin));
    }

    let (api_router, mut openapi) = router.split_for_parts();

    // Anonymous feedback lives on its own sub-router so it can carry the
    // strictest rate bucket without touching everything else.
    let (feedback_router, feedback_doc) = OpenApiRouter::new()
        .routes(routes!(feedback::create_feedback))
        .split_for_parts();
    openapi.merge(feedback_doc);
    let api_router = api_router.merge(feedback_router.layer(
        crate::auth::rate_limit::feedback_governor_layer(&cfg.rate_limit),
    ));
    (api_router, openapi)
}
