// ============================================================
// Auto.ml CLI — Persona Commands
//
// automl persona list    — List available review personas (from API)
// automl persona show    — Show persona details + system_prompt
// automl persona create  — Upload a custom persona (from local .md file)
// ============================================================

use anyhow::{Context, Result};
use console::style;
use std::fs;
use std::path::Path;

use crate::cli::{OutputFormat, PersonaCommands};
use crate::client::ApiClient;
use crate::output;

pub fn handle(action: PersonaCommands, format: &OutputFormat, client: &ApiClient) -> Result<()> {
    match action {
        PersonaCommands::List => list(format, client),
        PersonaCommands::Show { id } => show(&id, format, client),
        PersonaCommands::Create { name, file, public } => {
            create(&name, &file, public, format, client)
        }
    }
}

// ── List personas (sorted by usage frequency) ───────────────

fn list(format: &OutputFormat, client: &ApiClient) -> Result<()> {
    let result = client.get("/api/v1/personas")?;

    match format {
        OutputFormat::Json => output::print_output(format, &result),
        _ => {
            let personas = result["personas"].as_array();
            let count = result["count"].as_u64().unwrap_or(0);

            println!();
            println!(
                "  {} Review Personas — {} available",
                style("🎭").bold(),
                style(count).cyan()
            );
            println!();

            if let Some(personas) = personas {
                if personas.is_empty() {
                    output::info("No personas found. Upload one with: automl persona create --name \"My Reviewer\" --file soul.md");
                } else {
                    // Table header
                    println!(
                        "  {:<20} {:<25} {:<10} {}",
                        style("ID").bold().dim(),
                        style("Name").bold(),
                        style("Public").bold().dim(),
                        style("Tags").bold().dim(),
                    );
                    println!("  {}", style("─".repeat(70)).dim());

                    for p in personas {
                        let id = p["id"].as_str().unwrap_or("?");
                        let name = p["name"].as_str().unwrap_or("?");
                        let is_public = p["is_public"].as_bool().unwrap_or(false);
                        let tags: Vec<String> = p["style_tags"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();

                        println!(
                            "  {:<20} {:<25} {:<10} {}",
                            style(id).dim(),
                            style(name).yellow(),
                            if is_public {
                                style("✓").green()
                            } else {
                                style("✗").dim()
                            },
                            style(tags.join(", ")).dim(),
                        );
                    }
                }
            }
            println!();
        }
    }
    Ok(())
}

// ── Show persona details ────────────────────────────────────

fn show(id: &str, format: &OutputFormat, client: &ApiClient) -> Result<()> {
    // Fetch all personas and find by id or name
    let result = client.get("/api/v1/personas")?;
    let personas = result["personas"].as_array();

    let persona = personas.and_then(|list| {
        list.iter()
            .find(|p| p["id"].as_str() == Some(id) || p["name"].as_str() == Some(id))
    });

    match persona {
        Some(p) => match format {
            OutputFormat::Json => output::print_output(format, p),
            _ => {
                println!();
                println!(
                    "  {} Persona: {}",
                    style("🎭").bold(),
                    style(p["name"].as_str().unwrap_or("?")).bold().yellow()
                );
                println!();
                println!(
                    "  {} {}",
                    style("ID:").dim(),
                    p["id"].as_str().unwrap_or("?")
                );
                if let Some(desc) = p["description"].as_str() {
                    println!("  {} {}", style("Description:").dim(), desc);
                }
                let tags: Vec<String> = p["style_tags"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                if !tags.is_empty() {
                    println!("  {} {}", style("Tags:").dim(), tags.join(", "));
                }
                if let Some(prompt) = p["system_prompt"].as_str() {
                    println!();
                    println!("  {}", style("─── System Prompt ───").bold());
                    println!();
                    for line in prompt.lines().take(30) {
                        println!("  {}", line);
                    }
                    if prompt.lines().count() > 30 {
                        println!(
                            "  {} ({} more lines...)",
                            style("...").dim(),
                            prompt.lines().count() - 30
                        );
                    }
                }
                println!();
            }
        },
        None => {
            output::error(&format!(
                "Persona '{}' not found. Use `automl persona list` to see available personas.",
                id
            ));
        }
    }
    Ok(())
}

// ── Create / upload persona ─────────────────────────────────

fn create(
    name: &str,
    file: &str,
    is_public: bool,
    _format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    let path = Path::new(file);
    if !path.exists() {
        anyhow::bail!(
            "File not found: {}. Provide a .md file with the persona system prompt.",
            file
        );
    }

    let system_prompt =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", file))?;

    println!();
    println!("  {} Upload Persona", style("🎭").bold().cyan());
    println!();
    println!("  {} {}", style("Name:").dim(), style(name).bold().yellow());
    println!("  {} {}", style("File:").dim(), file);
    println!(
        "  {} {} lines",
        style("Prompt:").dim(),
        system_prompt.lines().count()
    );
    println!(
        "  {} {}",
        style("Public:").dim(),
        if is_public { "yes" } else { "no" }
    );
    println!();

    let body = serde_json::json!({
        "name": name,
        "description": format!("Custom persona uploaded from {}", path.file_name().unwrap_or_default().to_string_lossy()),
        "system_prompt": system_prompt,
        "style_tags": [],
        "is_public": is_public,
    });

    let result = client.post("/api/v1/personas", &body);

    match result {
        Ok(resp) => {
            let persona_id = resp["persona_id"].as_str().unwrap_or("?");
            output::success(&format!("Persona created: {}", persona_id));
            output::info(&format!(
                "Use in review: automl review auto <PAPER_ID> --persona {}",
                name
            ));
            println!();
        }
        Err(e) => {
            output::error(&format!("Failed to create persona: {e}"));
            output::info("Make sure you are logged in: automl auth login");
        }
    }

    Ok(())
}
