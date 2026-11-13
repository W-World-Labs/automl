// ============================================================
// Auto.ml CLI — Revise Command (Minor/Major Revision)
//
// After an editor decision of MINOR_REVISION or MAJOR_REVISION,
// authors use this command to resubmit their revised paper.
// ============================================================

use anyhow::{Context, Result};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::Path;

use crate::cli::OutputFormat;
use crate::client::ApiClient;
use crate::output;

pub fn handle(
    paper_id: &str,
    dir: &str,
    message: Option<String>,
    yes: bool,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    println!();
    println!(
        "  {}",
        style("🔄 Auto.ml — Revision Submission").bold().cyan()
    );
    println!();

    // ── Step 1: Check paper status ──────────────────────────
    println!("  {} Checking paper status...", style("→").dim());

    let paper_resp = client.get(&format!("/api/v1/papers/{paper_id}"));
    let paper = match paper_resp {
        Ok(resp) => {
            if let Some(p) = resp.get("paper") {
                p.clone()
            } else {
                anyhow::bail!("Paper not found: {paper_id}");
            }
        }
        Err(e) => {
            anyhow::bail!("Failed to fetch paper: {e}");
        }
    };

    let p = if paper["paper"].is_object() {
        &paper["paper"]
    } else {
        &paper
    };
    let status = p["status"].as_str().unwrap_or("UNKNOWN");
    let title = p["title"].as_str().unwrap_or("Untitled");

    // Determine revision type from status
    let revision_type = match status {
        "REBUTTAL" => "minor",     // Minor revision stays in REBUTTAL
        "UNDER_REVIEW" => "major", // Major revision reverts to UNDER_REVIEW
        "ACCEPTED" | "REJECTED" => {
            anyhow::bail!("Paper {paper_id} has already been {status}. Cannot submit revisions.");
        }
        _ => {
            anyhow::bail!(
                "Paper {paper_id} is in {status} stage — revisions can only be submitted \
                 when paper is in REBUTTAL (小修) or UNDER_REVIEW (大修) status."
            );
        }
    };

    let rev_label = if revision_type == "minor" {
        style("Minor Revision 小修 (内容修改)").yellow().bold()
    } else {
        style("Major Revision 大修 (补实验)").red().bold()
    };

    println!();
    println!("  {} {}", style("Paper:").dim(), title);
    println!("  {} {}", style("ID:").dim(), paper_id);
    println!("  {} {}", style("Revision Type:").dim(), rev_label);
    println!();

    // ── Step 2: Scan project directory ──────────────────────
    let project_dir = Path::new(dir);
    if !project_dir.is_dir() {
        anyhow::bail!("'{}' is not a directory", dir);
    }

    println!("  {} Scanning revised project...", style("→").dim());
    println!();

    let paper_tex = project_dir.join("paper.tex");
    let paper_md = project_dir.join("paper.md");
    let reproduce_yaml = project_dir.join("reproduce.yaml");
    let changelog = project_dir.join("CHANGELOG.md");
    let response_letter = project_dir.join("response_to_reviewers.md");
    let diff_file = project_dir.join("diff.pdf");

    // Paper source (required)
    let paper_file = if paper_tex.exists() {
        output::success(&format!("paper.tex found ({})", file_size(&paper_tex)?));
        Some(paper_tex.clone())
    } else if paper_md.exists() {
        output::success(&format!("paper.md found ({})", file_size(&paper_md)?));
        Some(paper_md.clone())
    } else {
        output::error("No paper.tex or paper.md found!");
        None
    };

    // reproduce.yaml (required for major, recommended for minor)
    if reproduce_yaml.exists() {
        output::success("reproduce.yaml found");
    } else if revision_type == "major" {
        output::error("reproduce.yaml not found! Required for major revision.");
        anyhow::bail!(
            "Major revision requires reproduce.yaml — you must include updated experiments."
        );
    } else {
        output::info("reproduce.yaml not found (optional for minor revision)");
    }

    // Response to reviewers (recommended)
    if response_letter.exists() {
        output::success(&format!(
            "response_to_reviewers.md found ({})",
            file_size(&response_letter)?
        ));
    } else {
        output::info("response_to_reviewers.md not found (recommended: point-by-point response)");
    }

    // Changelog
    if changelog.exists() {
        output::success(&format!("CHANGELOG.md found ({})", file_size(&changelog)?));
    }

    // Diff PDF
    if diff_file.exists() {
        output::success(&format!("diff.pdf found ({})", file_size(&diff_file)?));
    }

    // Check paper source exists
    if paper_file.is_none() {
        anyhow::bail!("Cannot submit revision: no paper.tex or paper.md found in '{dir}'");
    }

    // ── Step 3: Read response content ──────────────────────
    let response_content = if response_letter.exists() {
        fs::read_to_string(&response_letter).unwrap_or_default()
    } else if let Some(ref msg) = message {
        msg.clone()
    } else {
        String::new()
    };

    // ── Step 4: Summary ─────────────────────────────────────
    println!();
    println!("  {}", style("📋 Revision Submission Summary:").bold());
    println!("  {} {}", style("Paper:").dim(), title);
    println!("  {} {}", style("ID:").dim(), paper_id);
    println!(
        "  {} {}",
        style("Type:").dim(),
        if revision_type == "minor" {
            "Minor (小修)"
        } else {
            "Major (大修)"
        }
    );
    if !response_content.is_empty() {
        let preview = if response_content.len() > 80 {
            format!("{}...", &response_content[..80])
        } else {
            response_content.clone()
        };
        println!("  {} {}", style("Response:").dim(), preview);
    }
    println!();

    // ── Step 5: Confirm ─────────────────────────────────────
    if !yes {
        let confirm = dialoguer::Confirm::new()
            .with_prompt("  Confirm revision submission?")
            .default(true)
            .interact()?;

        if !confirm {
            output::info("Revision submission cancelled.");
            return Ok(());
        }
    }

    // ── Step 6: Submit rebuttal/revision ─────────────────────
    println!();
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈")
            .template("  {spinner:.cyan} {msg}")?,
    );
    pb.set_message("Submitting revision to auto.ml...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    // Submit rebuttal content
    let rebuttal_body = serde_json::json!({
        "content": if response_content.is_empty() {
            format!("[{} revision submitted via CLI]", if revision_type == "minor" { "Minor" } else { "Major" })
        } else {
            response_content
        },
        "revision_type": revision_type,
    });

    let rebuttal_result = client.post(
        &format!("/api/v1/papers/{paper_id}/rebuttal"),
        &rebuttal_body,
    );

    pb.finish_and_clear();

    match rebuttal_result {
        Ok(resp) => {
            let round = resp["round"].as_u64().unwrap_or(0);
            let content_length = resp["content_length"].as_u64().unwrap_or(0);

            println!();
            output::success(&format!(
                "Revision submitted successfully! (Round {round}, {content_length} chars)"
            ));

            if revision_type == "minor" {
                output::info("Minor revision (小修): Paper stays in REBUTTAL. Editor will review your changes.");
                output::info(&format!("Track progress: automl status {paper_id} --watch"));
            } else {
                output::info(
                    "Major revision (大修): Paper returned to UNDER_REVIEW for re-review.",
                );
                output::info("New reviewers may be assigned by the editor.");
                output::info(&format!("Track progress: automl status {paper_id} --watch"));
            }

            match format {
                OutputFormat::Json => output::print_output(format, &resp),
                OutputFormat::Minimal => println!("{paper_id}"),
                _ => {}
            }
        }
        Err(e) => {
            output::error(&format!("Revision submission failed: {e}"));
            output::info("Make sure the paper is in a revisable state (REBUTTAL or UNDER_REVIEW).");
        }
    }

    println!();
    Ok(())
}

fn file_size(path: &Path) -> Result<String> {
    let meta = fs::metadata(path).context("Failed to read file metadata")?;
    let bytes = meta.len();
    if bytes > 1024 * 1024 {
        Ok(format!("{:.1}MB", bytes as f64 / 1024.0 / 1024.0))
    } else if bytes > 1024 {
        Ok(format!("{:.1}KB", bytes as f64 / 1024.0))
    } else {
        Ok(format!("{}B", bytes))
    }
}
