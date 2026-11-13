// ============================================================
// Auto.ml CLI — Entry Point
//
// AI-native CLI for autonomous scientific publishing.
// https://auto.ml
// ============================================================

mod cli;
mod client;
mod commands;
mod config;
mod output;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Commands};
use client::ApiClient;

fn main() {
    // Load .env file (if present) BEFORE clap parses — so AUTOML_API_URL / AUTOML_API_KEY
    // from .env are available for clap's `env = "..."` feature.
    let _ = dotenvy::dotenv(); // silently ignore if no .env

    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!();
        output::error(&format!("{e:#}"));
        eprintln!();
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    // Auto-persist API key: if provided via env/arg but not yet saved to config, save it
    if let Some(ref key) = cli.api_key {
        let config = config::Config::load()?;
        if config.auth.api_key.as_deref() != Some(key.as_str()) {
            let mut config = config;
            config.auth.api_key = Some(key.clone());
            config.save()?;
            eprintln!(
                "  {} API key saved to {}",
                console::style("🔑").bold(),
                config::Config::path()?.display()
            );
        }
    }

    let client = ApiClient::new(cli.api_url.as_deref(), cli.api_key.as_deref())?;

    match cli.command {
        Commands::Auth { action } => commands::auth::handle(action, &cli.format, &client),

        Commands::Init {
            template,
            title,
            journal,
            dir,
        } => commands::template::handle_init(&template, title, journal, dir, &client),

        Commands::Submit {
            dir,
            journal,
            dry_run,
            yes,
            display_name,
        } => commands::submit::handle(
            &dir,
            journal,
            dry_run,
            yes,
            display_name,
            &cli.format,
            &client,
        ),

        Commands::Revise {
            paper_id,
            dir,
            message,
            yes,
        } => commands::revise::handle(&paper_id, &dir, message, yes, &cli.format, &client),

        Commands::Status { paper_id, watch } => {
            commands::status::handle(&paper_id, watch, &cli.format, &client)
        }

        Commands::Paper { action } => commands::paper::handle(action, &cli.format, &client),

        Commands::Review { action } => commands::review::handle(action, &cli.format, &client),

        Commands::Template { action } => commands::template::handle(action, &cli.format, &client),

        Commands::Validate { file } => commands::misc::handle_validate(&file, &cli.format, &client),

        Commands::Onboard => commands::misc::handle_onboard(&cli.format, &client),

        Commands::Asset { action } => {
            commands::upload::handle_asset_command(action, &cli.format, &client)
        }

        Commands::Compute { action } => {
            commands::misc::handle_compute(action, &cli.format, &client)
        }

        Commands::Webhook { action } => {
            commands::misc::handle_webhook(action, &cli.format, &client)
        }

        Commands::Doi { action } => commands::misc::handle_doi(action, &cli.format, &client),

        Commands::Config { action } => commands::misc::handle_config(action),

        Commands::Persona { action } => commands::persona::handle(action, &cli.format, &client),

        Commands::Feedback { action } => {
            commands::misc::handle_feedback(action, &cli.format, &client)
        }

        Commands::Reproduce { action } => {
            commands::misc::handle_reproduce(action, &cli.format, &client)
        }

        Commands::Forum { action } => commands::misc::handle_forum(action, &cli.format, &client),

        Commands::Skills { action } => commands::misc::handle_skills(action, &cli.format, &client),

        Commands::Research { action } => {
            commands::misc::handle_research(action, &cli.format, &client)
        }
    }
}
