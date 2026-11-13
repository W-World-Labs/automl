// ============================================================
// Auto.ml CLI — Paper Commands
// ============================================================

use anyhow::Result;

use crate::cli::{OutputFormat, PaperCommands};
use crate::client::ApiClient;
use crate::output;

pub fn handle(action: PaperCommands, format: &OutputFormat, client: &ApiClient) -> Result<()> {
    match action {
        PaperCommands::List {
            all,
            status,
            journal,
            limit,
        } => list(all, status, journal, limit, format, client),
        PaperCommands::Get { paper_id } => get(&paper_id, format, client),
        PaperCommands::Search { query, domain } => search(&query, domain, format, client),
        PaperCommands::Advance { paper_id } => advance(&paper_id, client),
        PaperCommands::Decision {
            paper_id,
            verdict,
            reason,
        } => decision(&paper_id, &verdict, &reason, format, client),
        PaperCommands::Retract {
            paper_id,
            action,
            reason,
        } => retract(&paper_id, &action, &reason, client),
        PaperCommands::Comment { paper_id, comment } => {
            post_comment(&paper_id, &comment, format, client)
        }
    }
}

fn list(
    all: bool,
    status: Option<String>,
    journal: Option<String>,
    limit: u32,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    let limit_str = limit.to_string();
    let mut query: Vec<(&str, &str)> = vec![("limit", &limit_str)];
    if let Some(ref s) = status {
        query.push(("status", s.as_str()));
    }
    if let Some(ref j) = journal {
        query.push(("journal", j.as_str()));
    }

    // Default: show only my papers (by scholar_id from config)
    // --all: show all papers on the platform
    let config = crate::config::Config::load().unwrap_or_default();
    let scholar_id_owned = config.auth.scholar_id.clone().unwrap_or_default();
    let filtering_mine = !all && !scholar_id_owned.is_empty();
    if filtering_mine {
        query.push(("author_id", scholar_id_owned.as_str()));
    }

    let result = client.get_with_params("/api/v1/papers", &query)?;

    match format {
        OutputFormat::Json => output::print_output(format, &result),
        OutputFormat::Minimal => {
            if let Some(papers) = result.as_array() {
                for p in papers {
                    println!("{}", p["id"].as_str().unwrap_or("?"));
                }
            }
        }
        OutputFormat::Table => {
            if filtering_mine {
                output::info("Showing your papers. Use --all to see all papers on the platform.");
            }
            if let Some(papers) = result.as_array() {
                output::print_papers_table(papers);
            } else if let Some(papers) = result["papers"].as_array() {
                output::print_papers_table(papers);
            } else {
                output::print_output(format, &result);
            }
        }
    }
    Ok(())
}

fn get(paper_id: &str, format: &OutputFormat, client: &ApiClient) -> Result<()> {
    let result = client.get(&format!("/api/v1/papers/{paper_id}"))?;

    match format {
        OutputFormat::Json => output::print_output(format, &result),
        OutputFormat::Minimal => {
            println!("{}", result["status"].as_str().unwrap_or("?"));
        }
        OutputFormat::Table => {
            output::print_paper_detail(&result);
        }
    }
    Ok(())
}

fn search(
    query: &str,
    domain: Option<String>,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    let mut params: Vec<(&str, &str)> = vec![("q", query)];
    let domain_ref;
    if let Some(ref d) = domain {
        domain_ref = d.as_str();
        params.push(("domain", domain_ref));
    }

    let result = client.get_with_params("/api/v1/papers", &params)?;

    match format {
        OutputFormat::Json => output::print_output(format, &result),
        _ => {
            if let Some(papers) = result["papers"].as_array() {
                if papers.is_empty() {
                    output::info("No papers found.");
                } else {
                    output::print_papers_table(papers);
                }
            }
            if let Some(scholars) = result["scholars"].as_array() {
                if !scholars.is_empty() {
                    println!();
                    output::info(&format!("{} scholars matched", scholars.len()));
                }
            }
        }
    }
    Ok(())
}

fn advance(paper_id: &str, client: &ApiClient) -> Result<()> {
    let result = client.post(
        &format!("/api/v1/papers/{paper_id}/advance"),
        &serde_json::json!({}),
    )?;

    let new_status = result["new_status"].as_str().unwrap_or("?");
    output::success(&format!("Paper {paper_id} advanced to: {new_status}"));
    Ok(())
}

fn decision(
    paper_id: &str,
    verdict: &str,
    reason: &str,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    let normalized = verdict.to_lowercase();
    if !matches!(normalized.as_str(), "accept" | "reject") {
        anyhow::bail!(
            "Invalid verdict: '{}'. Must be 'accept' or 'reject'.",
            verdict
        );
    }

    output::info(&format!(
        "Submitting decision for {paper_id}: {}",
        normalized.to_uppercase()
    ));

    let body = serde_json::json!({
        "verdict": normalized,
        "reason": reason,
    });
    let result = client.post(&format!("/api/v1/papers/{paper_id}/decision"), &body)?;

    let new_status = result["new_status"]
        .as_str()
        .or_else(|| result["status"].as_str())
        .unwrap_or("?");
    let doi = result["doi"].as_str().unwrap_or("");

    match normalized.as_str() {
        "accept" => {
            output::success(&format!("Paper {paper_id} ACCEPTED!"));
            if !doi.is_empty() {
                output::success(&format!("DOI: {doi}"));
            }
        }
        "reject" => {
            output::info(&format!("Paper {paper_id} REJECTED. Status: {new_status}"));
        }
        _ => {}
    }

    match format {
        OutputFormat::Json => output::print_output(format, &result),
        _ => {}
    }
    Ok(())
}

fn retract(paper_id: &str, action: &str, reason: &str, client: &ApiClient) -> Result<()> {
    let body = serde_json::json!({
        "action": action,
        "reason": reason,
    });
    let result = client.post(&format!("/api/v1/papers/{paper_id}/retract"), &body)?;
    output::success(&format!("Paper {paper_id}: {action}"));
    output::print_output(&OutputFormat::Json, &result);
    Ok(())
}

fn post_comment(
    paper_id: &str,
    comment: &str,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    let body = serde_json::json!({
        "content": comment,
    });
    let result = client.post(&format!("/api/v1/papers/{paper_id}/comment"), &body)?;

    match format {
        OutputFormat::Json => output::print_output(format, &result),
        _ => {
            let cid = result["comment_id"].as_str().unwrap_or("?");
            output::success(&format!("Comment posted on {paper_id} (id: {cid})"));
        }
    }
    Ok(())
}
