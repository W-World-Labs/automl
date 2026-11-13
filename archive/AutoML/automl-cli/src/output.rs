// ============================================================
// Auto.ml CLI — Output Formatting
// ============================================================

use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color, Table};
use console::style;

use crate::cli::OutputFormat;

/// Print data in the requested output format
pub fn print_output(format: &OutputFormat, json_value: &serde_json::Value) {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(json_value).unwrap_or_default()
            );
        }
        OutputFormat::Minimal => {
            // Print just the ID or key field
            if let Some(id) = json_value.get("paper_id").or(json_value.get("id")) {
                println!("{}", id.as_str().unwrap_or(&id.to_string()));
            } else {
                println!("{json_value}");
            }
        }
        OutputFormat::Table => {
            // Fallback: pretty-print JSON if no table formatter handles it
            println!(
                "{}",
                serde_json::to_string_pretty(json_value).unwrap_or_default()
            );
        }
    }
}

/// Print a list of papers as a formatted table
pub fn print_papers_table(papers: &[serde_json::Value]) {
    if papers.is_empty() {
        println!("{}", style("No papers found.").dim());
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec![
            Cell::new("ID").fg(Color::Cyan),
            Cell::new("Title").fg(Color::Cyan),
            Cell::new("Status").fg(Color::Cyan),
            Cell::new("Journal").fg(Color::Cyan),
            Cell::new("Score").fg(Color::Cyan),
        ]);

    for paper in papers {
        let status = paper["status"].as_str().unwrap_or("?");
        let status_cell = Cell::new(status).fg(match status {
            "ACCEPTED" => Color::Green,
            "REJECTED" | "RETRACTED" => Color::Red,
            "UNDER_REVIEW" => Color::Blue,
            "REBUTTAL" => Color::Yellow,
            _ => Color::White,
        });

        table.add_row(vec![
            Cell::new(paper["id"].as_str().unwrap_or("?")),
            Cell::new(truncate(paper["title"].as_str().unwrap_or("?"), 45)),
            status_cell,
            Cell::new(paper["journal_slug"].as_str().unwrap_or("?")),
            Cell::new(format!(
                "{}/100",
                paper["repro_score"].as_u64().unwrap_or(0)
            )),
        ]);
    }

    println!("{table}");
}

/// Print paper detail view with pipeline status
pub fn print_paper_detail(paper: &serde_json::Value) {
    let id = paper["id"].as_str().unwrap_or("?");
    let title = paper["title"].as_str().unwrap_or("?");
    let status = paper["status"].as_str().unwrap_or("?");

    println!();
    println!("  {} {}", style("Paper:").bold().cyan(), style(id).bold());
    println!("  {} {}", style("Title:").bold(), title);
    println!("  {} {}", style("Status:").bold(), colorize_status(status));
    println!();

    // Pipeline visualization
    let stages = [
        "SUBMITTED",
        "REFERENCE_CHECK",
        "REPRODUCIBILITY_GATE",
        "UNDER_REVIEW",
        "REBUTTAL",
    ];
    let terminal = ["ACCEPTED", "REJECTED"];
    print!("  ");
    let mut found_current = false;
    for (i, stage) in stages.iter().enumerate() {
        if i > 0 {
            print!(" → ");
        }
        if *stage == status {
            print!("{}", style(format!("● {stage}")).bold().cyan());
            found_current = true;
        } else if !found_current {
            print!("{}", style(format!("✓ {stage}")).green());
        } else {
            print!("{}", style(format!("○ {stage}")).dim());
        }
    }
    if terminal.contains(&status) {
        print!(" → {}", colorize_status(status));
    }
    println!();
    println!();

    // Details
    if let Some(domain) = paper["domain"].as_str() {
        println!("  {} {}", style("Domain:").dim(), domain);
    }
    if let Some(journal) = paper["journal_slug"].as_str() {
        println!("  {} {}", style("Journal:").dim(), journal);
    }
    if let Some(score) = paper["repro_score"].as_u64() {
        println!("  {} {}/100", style("Repro Score:").dim(), score);
    }
    if let Some(repo) = paper["github_repo"].as_str() {
        println!("  {} {}", style("GitHub:").dim(), repo);
    }
    if let Some(doi) = paper["doi"].as_str() {
        println!("  {} {}", style("DOI:").dim(), doi);
    }
    println!();
}

/// Print review debate feed
pub fn print_review_feed(review: &serde_json::Value) {
    if let Some(feed) = review["debate_feed"].as_array() {
        for entry in feed {
            print_review_entry(entry, 0);
        }
    }
}

fn print_review_entry(entry: &serde_json::Value, depth: usize) {
    let indent = "  ".repeat(depth + 1);
    let persona = entry["persona"].as_str().unwrap_or("?");
    let verdict = entry["verdict"].as_str().unwrap_or("?");
    let content = entry["content"].as_str().unwrap_or("");

    println!();
    println!(
        "{indent}{} {} — {}",
        style("▸").bold(),
        style(persona).bold().cyan(),
        colorize_status(verdict)
    );
    // Word-wrap content at 72 chars
    for line in textwrap(content, 72) {
        println!("{indent}  {}", style(line).dim());
    }

    if let Some(replies) = entry["replies"].as_array() {
        for reply in replies {
            print_review_entry(reply, depth + 1);
        }
    }
}

fn colorize_status(status: &str) -> console::StyledObject<&str> {
    match status {
        "ACCEPT" | "ACCEPTED" => style(status).green().bold(),
        "REJECT" | "REJECTED" | "RETRACTED" => style(status).red().bold(),
        "MAJOR_REVISION" | "REBUTTAL" => style(status).yellow().bold(),
        "MINOR_REVISION" | "BORDERLINE" => style(status).yellow(),
        "UNDER_REVIEW" => style(status).blue(),
        _ => style(status).white(),
    }
}

/// Colorize status — returns a styled string (owned)
pub fn colorize_status_str(status: &str) -> console::StyledObject<String> {
    match status {
        "ACCEPT" | "ACCEPTED" => style(status.to_string()).green().bold(),
        "REJECT" | "REJECTED" | "RETRACTED" => style(status.to_string()).red().bold(),
        "MAJOR_REVISION" | "REBUTTAL" => style(status.to_string()).yellow().bold(),
        "MINOR_REVISION" | "BORDERLINE" => style(status.to_string()).yellow(),
        "UNDER_REVIEW" => style(status.to_string()).blue(),
        _ => style(status.to_string()).white(),
    }
}

/// Print a success message
pub fn success(msg: &str) {
    println!("  {} {}", style("✓").green().bold(), msg);
}

/// Print an error message
pub fn error(msg: &str) {
    eprintln!("  {} {}", style("✗").red().bold(), msg);
}

/// Print an info message
pub fn info(msg: &str) {
    println!("  {} {}", style("→").cyan(), msg);
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max - 3])
    } else {
        s.to_string()
    }
}

fn textwrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.len() + word.len() + 1 > width {
            lines.push(current.clone());
            current.clear();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
