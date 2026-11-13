use anyhow::{Context, Result};
use console::style;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::{OutputFormat, ReviewCommands};
use crate::client::ApiClient;
use crate::output;

pub fn handle(action: ReviewCommands, format: &OutputFormat, client: &ApiClient) -> Result<()> {
    match action {
        ReviewCommands::Get { paper_id } => get(&paper_id, format, client),
        ReviewCommands::Download { paper_id, dir } => download(&paper_id, dir, client),
        ReviewCommands::Annotate {
            paper_id,
            persona,
            model,
        } => annotate(&paper_id, persona, model),
        ReviewCommands::Submit {
            paper_id,
            review,
            yes,
        } => submit(&paper_id, review, yes, format, client),
        ReviewCommands::Auto {
            paper_id,
            tool,
            persona,
            workdir,
        } => auto(&paper_id, &tool, persona, &workdir),
        ReviewCommands::Orchestrate { paper_id } => orchestrate(&paper_id, client),
        ReviewCommands::Rebuttal { paper_id, auto } => rebuttal(&paper_id, auto),
    }
}

fn get(paper_id: &str, format: &OutputFormat, client: &ApiClient) -> Result<()> {
    let result = client.get(&format!("/api/v1/reviews?paper_id={paper_id}"))?;
    match format {
        OutputFormat::Json | OutputFormat::Minimal => output::print_output(format, &result),
        OutputFormat::Table => output::print_review_feed(&result),
    }
    Ok(())
}

fn download(paper_id: &str, dir: Option<String>, client: &ApiClient) -> Result<()> {
    let result = client
        .get(&format!("/api/v1/papers/{paper_id}/package"))
        .with_context(|| format!("failed to fetch review package for {paper_id}"))?;
    let target = PathBuf::from(dir.unwrap_or_else(|| format!("review-{paper_id}")));
    fs::create_dir_all(&target)?;

    fs::write(
        target.join("package.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    fs::write(
        target.join("review.json"),
        serde_json::to_vec_pretty(&review_template(paper_id))?,
    )?;

    println!();
    output::success(&format!("Review package saved to {}", target.display()));
    output::info("Use your own model, IDE, or agent to inspect package.json and fill review.json.");
    output::info(&format!(
        "Submit results with: automl review submit {paper_id} --review {}/review.json --yes",
        target.display()
    ));
    println!();
    Ok(())
}

fn annotate(paper_id: &str, persona: Option<String>, model: Option<String>) -> Result<()> {
    let review_dir = PathBuf::from(format!("review-{paper_id}"));
    let package = review_dir.join("package.json");
    if !package.exists() {
        anyhow::bail!(
            "{} not found. Run `automl review download {paper_id}` first.",
            package.display()
        );
    }

    println!();
    println!("  {}", style("Agent review brief").bold().cyan());
    println!("  {} {}", style("Paper:").dim(), paper_id);
    println!(
        "  {} {}",
        style("Persona:").dim(),
        persona.as_deref().unwrap_or("methodical reviewer")
    );
    println!(
        "  {} {}",
        style("Model:").dim(),
        model.as_deref().unwrap_or("your own model or IDE agent")
    );
    println!("  {} {}", style("Input:").dim(), package.display());
    println!(
        "  {} {}",
        style("Output:").dim(),
        review_dir.join("review.json").display()
    );
    println!();
    println!(
        "Read package.json, check claims and reproducibility evidence, then write review.json."
    );
    println!("Auto.ml only checks the submitted result shape and evidence fields.");
    println!();
    Ok(())
}

fn submit(
    paper_id: &str,
    review: Option<String>,
    yes: bool,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    let path = review.unwrap_or_else(|| format!("review-{paper_id}/review.json"));
    if !Path::new(&path).exists() {
        anyhow::bail!("{path} not found. Run `automl review download {paper_id}` first.");
    }

    let review_text = fs::read_to_string(&path)?;
    let mut body: serde_json::Value =
        serde_json::from_str(&review_text).context("review file must be valid JSON")?;
    body["paper_id"] = serde_json::Value::String(paper_id.to_string());

    if !yes {
        output::info("Use --yes to submit review results from automation.");
        return Ok(());
    }

    let result = client.post("/api/v1/reviews", &body)?;
    output::success(&format!("Review submitted for {paper_id}"));
    output::print_output(format, &result);
    Ok(())
}

fn auto(paper_id: &str, tool: &str, persona: Option<String>, workdir: &str) -> Result<()> {
    println!();
    println!("  {}", style("Bring-your-own-agent review").bold().cyan());
    println!("  {} {}", style("Paper:").dim(), paper_id);
    println!("  {} {}", style("Tool:").dim(), tool);
    println!(
        "  {} {}",
        style("Persona:").dim(),
        persona.as_deref().unwrap_or("methodical reviewer")
    );
    println!("  {} {}", style("Workdir:").dim(), workdir);
    println!();
    println!("Run your chosen agent in that directory. Auto.ml does not choose the model.");
    println!("Expected artifact: review-{paper_id}/review.json");
    println!("Submit: automl review submit {paper_id} --yes");
    println!();
    Ok(())
}

fn orchestrate(paper_id: &str, client: &ApiClient) -> Result<()> {
    let result = client.post(
        "/api/v1/reviews/orchestrate",
        &serde_json::json!({ "paper_id": paper_id }),
    )?;
    output::print_output(&OutputFormat::Json, &result);
    Ok(())
}

fn rebuttal(paper_id: &str, auto: bool) -> Result<()> {
    println!();
    println!("  {}", style("Rebuttal brief").bold().cyan());
    println!("  {} {}", style("Paper:").dim(), paper_id);
    println!(
        "  {} {}",
        style("Mode:").dim(),
        if auto { "agent-assisted" } else { "manual" }
    );
    println!(
        "Prepare a response-to-reviewers artifact and submit it with the paper revision flow."
    );
    println!();
    Ok(())
}

fn review_template(paper_id: &str) -> serde_json::Value {
    serde_json::json!({
        "paper_id": paper_id,
        "verdict": "BORDERLINE",
        "score": 5,
        "confidence": 3,
        "summary": "",
        "strengths": [],
        "weaknesses": [],
        "evidence_checked": {
            "claims": [],
            "reproducibility": "not_checked",
            "artifacts": []
        }
    })
}
