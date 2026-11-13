// ============================================================
// Auto.ml CLI — Status Command (with --watch)
// ============================================================

use anyhow::Result;
use console::style;

use crate::cli::OutputFormat;
use crate::client::ApiClient;
use crate::output;

pub fn handle(
    paper_id: &str,
    watch: bool,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    if watch {
        watch_loop(paper_id, format, client)
    } else {
        show_once(paper_id, format, client)
    }
}

fn show_once(paper_id: &str, format: &OutputFormat, client: &ApiClient) -> Result<()> {
    let result = client.get(&format!("/api/v1/papers/{paper_id}"))?;

    match format {
        OutputFormat::Json => output::print_output(format, &result),
        OutputFormat::Minimal => {
            let p = if result["paper"].is_object() {
                &result["paper"]
            } else {
                &result
            };
            println!("{}", p["status"].as_str().unwrap_or("?"));
        }
        OutputFormat::Table => {
            let p = if result["paper"].is_object() {
                &result["paper"]
            } else {
                &result
            };
            print_status_dashboard(p);
        }
    }
    Ok(())
}

fn watch_loop(paper_id: &str, format: &OutputFormat, client: &ApiClient) -> Result<()> {
    let mut last_status = String::new();

    loop {
        // Clear screen for dashboard
        print!("\x1B[2J\x1B[H");

        match client.get(&format!("/api/v1/papers/{paper_id}")) {
            Ok(result) => {
                let p = if result["paper"].is_object() {
                    &result["paper"]
                } else {
                    &result
                };
                let current_status = p["status"].as_str().unwrap_or("?").to_string();

                match format {
                    OutputFormat::Json => output::print_output(format, &result),
                    _ => print_status_dashboard(p),
                }

                // Notify on status change
                if !last_status.is_empty() && current_status != last_status {
                    println!();
                    println!(
                        "  {} Status changed: {} → {}",
                        style("🔔").bold(),
                        style(&last_status).dim(),
                        output::colorize_status_str(&current_status),
                    );
                }

                // Exit watch if terminal state
                if matches!(
                    current_status.as_str(),
                    "ACCEPTED" | "REJECTED" | "RETRACTED"
                ) {
                    println!();
                    println!(
                        "  {} Paper reached terminal state. Exiting watch.",
                        style("✓").green()
                    );
                    return Ok(());
                }

                last_status = current_status;
            }
            Err(e) => {
                output::error(&format!("Failed to fetch status: {e}"));
            }
        }

        println!();
        println!(
            "  {} Refreshing in 30s... (Ctrl+C to stop)",
            style("↻").dim()
        );

        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}

fn print_status_dashboard(paper: &serde_json::Value) {
    let id = paper["id"].as_str().unwrap_or("?");
    let title = paper["title"].as_str().unwrap_or("?");
    let status = paper["status"].as_str().unwrap_or("?");

    println!();
    println!(
        "  {}",
        style("╔═══════════════════════════════════════════════════════╗").cyan()
    );

    let display_title = format!("{:<38}", truncate_str(title, 38));
    println!(
        "  {}  {} — {}  {}",
        style("║").cyan(),
        style(id).bold(),
        display_title,
        style("║").cyan(),
    );

    // Status text formatting (ANSI strings affect length, so pad raw string then colorize)
    let raw_status = format!("Status: {}", status);
    let padded_status = format!("{:<51}", raw_status);
    let colored_status =
        padded_status.replace(status, &output::colorize_status_str(status).to_string());

    println!(
        "  {}  {}  {}",
        style("║").cyan(),
        colored_status,
        style("║").cyan()
    );
    println!(
        "  {}",
        style("╚═══════════════════════════════════════════════════════╝").cyan()
    );
    println!();

    // Pipeline visualization
    let stages = [
        ("SUBMITTED", "SUBMIT"),
        ("REFERENCE_CHECK", "REF_CHECK"),
        ("REPRODUCIBILITY_GATE", "REPRO"),
        ("UNDER_REVIEW", "REVIEW"),
        ("REBUTTAL", "REBUTTAL"),
    ];

    print!("  ");
    let mut passed_current = false;
    for (i, (stage_key, label)) in stages.iter().enumerate() {
        if i > 0 {
            print!(" → ");
        }
        if *stage_key == status {
            print!("{}", style(format!("● {label}")).bold().cyan());
            passed_current = true;
        } else if !passed_current {
            print!("{}", style(format!("✓ {label}")).green());
        } else {
            print!("{}", style(format!("○ {label}")).dim());
        }
    }

    // Terminal state
    match status {
        "ACCEPTED" => print!(" → {}", style("✓ ACCEPTED").green().bold()),
        "REJECTED" => print!(" → {}", style("✗ REJECTED").red().bold()),
        "RETRACTED" => print!(" → {}", style("✗ RETRACTED").red()),
        _ => {}
    }
    println!();
    println!();

    // Details
    if let Some(score) = paper["repro_score"].as_u64() {
        let bar = score_bar(score as u32);
        println!("  {} {} {}/100", style("Repro:").dim(), bar, score);
    }
    if let Some(repo) = paper["github_repo"].as_str() {
        println!("  {} {}", style("GitHub:").dim(), style(repo).underlined());
    }
    if let Some(doi) = paper["doi"].as_str() {
        println!("  {} {}", style("DOI:").dim(), doi);
    }
    if let Some(tier) = paper["compute_tier"].as_str() {
        println!("  {} {}", style("Compute:").dim(), tier);
    }
    println!();
}

fn score_bar(score: u32) -> String {
    let filled = (score / 10) as usize;
    let empty = 10 - filled;
    let color = if score >= 80 {
        "\x1b[32m"
    }
    // green
    else if score >= 50 {
        "\x1b[33m"
    }
    // yellow
    else {
        "\x1b[31m"
    }; // red
    format!(
        "{color}{}{}  \x1b[0m",
        "█".repeat(filled),
        "░".repeat(empty)
    )
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}...", s.chars().take(max - 3).collect::<String>())
    } else {
        s.to_string()
    }
}
