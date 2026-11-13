// ============================================================
// Auto.ml CLI — Template & Init Commands
// ============================================================

use anyhow::Result;
use console::style;
use std::fs;
use std::path::Path;

use crate::cli::{OutputFormat, TemplateCommands};
use crate::client::ApiClient;
use crate::output;

pub fn handle(action: TemplateCommands, format: &OutputFormat, client: &ApiClient) -> Result<()> {
    match action {
        TemplateCommands::List => list(format, client),
        TemplateCommands::Fork { template_id, title } => fork(&template_id, title, client),
        TemplateCommands::Download { template_id } => download(&template_id, client),
    }
}

fn list(format: &OutputFormat, client: &ApiClient) -> Result<()> {
    let result = client.get("/api/v1/templates")?;
    match format {
        OutputFormat::Json => output::print_output(format, &result),
        _ => {
            println!();
            println!("  {}", style("Available Templates").bold().cyan());
            println!();
            if let Some(templates) = result.as_array() {
                for t in templates {
                    let id = t["id"].as_str().unwrap_or("?");
                    let name = t["name"].as_str().unwrap_or("?");
                    let desc = t["description"].as_str().unwrap_or("");
                    println!("  {} {} — {}", style(id).bold(), style(name).cyan(), desc);
                }
            }
            println!();
        }
    }
    Ok(())
}

fn fork(template_id: &str, title: Option<String>, client: &ApiClient) -> Result<()> {
    let title = match title {
        Some(t) => t,
        None => dialoguer::Input::new()
            .with_prompt("  Paper title")
            .interact_text()?,
    };

    let body = serde_json::json!({
        "template_id": template_id,
        "title": title,
    });
    let result = client.post("/api/v1/templates/fork", &body)?;
    output::success(&format!("Forked template '{}' → '{}'", template_id, title));
    output::print_output(&OutputFormat::Json, &result);
    Ok(())
}

fn download(template_id: &str, client: &ApiClient) -> Result<()> {
    output::info(&format!("Downloading template '{template_id}'..."));
    let result = client.get(&format!("/api/v1/templates/{template_id}"))?;
    output::success("Template downloaded.");
    output::print_output(&OutputFormat::Json, &result);
    Ok(())
}

/// Handle `automl init` — scaffold a paper project by downloading the complete template
pub fn handle_init(
    _template: &str,
    title: Option<String>,
    journal_arg: Option<String>,
    dir: Option<String>,
    client: &ApiClient,
) -> Result<()> {
    println!();
    println!("  {}", style("📁 Initialize Paper Project").bold().cyan());
    println!();

    let paper_title = match title {
        Some(t) => t,
        None => dialoguer::Input::new()
            .with_prompt("  Paper title")
            .interact_text()?,
    };

    let valid_journals = [
        "kindergarten",
        "intelligence",
        "science",
        "life",
        "robotics",
        "security",
        "systems",
    ];

    let journal = match journal_arg {
        Some(j) => {
            let j_lower = j.to_lowercase();
            if !valid_journals.contains(&j_lower.as_str()) {
                anyhow::bail!(
                    "Invalid journal '{}'. Valid: {}",
                    j,
                    valid_journals.join(", ")
                );
            }
            j_lower
        }
        None => {
            // Interactive mode: show fuzzy select
            let journals = [
                "🎓 kindergarten (sandbox)",
                "🧠 intelligence",
                "⚛  science",
                "🧬 life",
                "🤖 robotics",
                "🔐 security",
                "🎮 systems",
            ];
            let journal_idx = dialoguer::FuzzySelect::new()
                .with_prompt("  Target journal")
                .items(&journals)
                .default(0)
                .interact()?;
            valid_journals[journal_idx].to_string()
        }
    };

    // Determine project directory
    let dir_name = dir.unwrap_or_else(|| {
        paper_title
            .to_lowercase()
            .replace(' ', "-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect()
    });
    let project_dir = Path::new(&dir_name);

    if project_dir.exists() && fs::read_dir(project_dir)?.next().is_some() {
        anyhow::bail!("Directory '{}' already exists and is not empty", dir_name);
    }
    fs::create_dir_all(project_dir)?;

    // Try downloading the complete template from server
    let template_url = format!("{}/paper-template.tar.gz", client.base_url());
    output::info(&format!(
        "Downloading template from {}...",
        style(&template_url).dim()
    ));

    let downloaded = download_and_extract_template(&template_url, project_dir);

    if let Err(e) = downloaded {
        eprintln!(
            "  {} Download failed ({}), generating locally...",
            style("⚠").yellow(),
            e
        );
        generate_fallback_template(project_dir, &paper_title, &journal, &dir_name)?;
    } else {
        // Patch title and journal into downloaded template
        patch_template(project_dir, &paper_title, &journal)?;
    }

    // Print result
    println!();
    output::success(&format!("Project created: {dir_name}/"));
    // List actual files
    print_tree(project_dir, "")?;
    println!();
    output::info(&format!("Journal: {journal}"));
    output::info("Next steps:");
    println!("    1. Edit paper.tex with your research");
    println!("    2. Upload reference PDFs: automl asset upload refs/ --type reference");
    println!("    3. Fill asset_ids in refs/refs-manifest.yaml");
    println!("    4. Submit: automl submit . --journal {}", journal);
    println!();
    Ok(())
}

/// Download template tar.gz and extract to project dir
fn download_and_extract_template(url: &str, project_dir: &Path) -> Result<()> {
    let resp = reqwest::blocking::get(url)?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }
    let bytes = resp.bytes()?;
    let size_kb = bytes.len() / 1024;
    output::info(&format!("Downloaded template ({size_kb}KB), extracting..."));

    // Write to temp file then extract with tar
    let tmp_file = project_dir.join(".template.tar.gz");
    fs::write(&tmp_file, &bytes)?;

    let status = std::process::Command::new("tar")
        .args(["xzf", tmp_file.to_str().unwrap(), "--strip-components=1"])
        .current_dir(project_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()?;

    // Clean up temp file
    let _ = fs::remove_file(&tmp_file);

    if !status.success() {
        anyhow::bail!("tar extraction failed");
    }
    Ok(())
}

/// Patch title and journal into the downloaded template files
fn patch_template(project_dir: &Path, title: &str, journal: &str) -> Result<()> {
    // Patch paper.tex: replace placeholder title
    let paper_path = project_dir.join("paper.tex");
    if paper_path.exists() {
        let content = fs::read_to_string(&paper_path)?;
        let patched = content
            .replace("Your Paper Title Here", title)
            .replace("Your Paper Title", title);
        fs::write(&paper_path, patched)?;
    }

    // Patch reproduce.yaml: replace title and journal
    let repro_path = project_dir.join("reproduce.yaml");
    if repro_path.exists() {
        let content = fs::read_to_string(&repro_path)?;
        let patched = content
            .replace(
                "title: \"Your Paper Title\"",
                &format!("title: \"{}\"", title),
            )
            .replace(
                "journal_slug: \"kindergarten\"",
                &format!("journal_slug: \"{}\"", journal),
            )
            .replace("journal: kindergarten", &format!("journal: {}", journal));
        fs::write(&repro_path, patched)?;
    }

    Ok(())
}

/// Print directory tree
fn print_tree(dir: &Path, prefix: &str) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| !e.file_name().to_str().unwrap_or("").starts_with('.'))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for (i, entry) in entries.iter().enumerate() {
        let is_last_entry = i == entries.len() - 1;
        let connector = if is_last_entry {
            "└── "
        } else {
            "├── "
        };
        let name = entry.file_name();
        let name_str = name.to_str().unwrap_or("?");

        if entry.path().is_dir() {
            println!("  {prefix}{connector}{name_str}/");
            let new_prefix = format!("{prefix}{}", if is_last_entry { "    " } else { "│   " });
            print_tree(&entry.path(), &new_prefix)?;
        } else {
            println!("  {prefix}{connector}{name_str}");
        }
    }
    Ok(())
}

/// Fallback: generate minimal template when download fails
fn generate_fallback_template(
    project_dir: &Path,
    title: &str,
    journal: &str,
    dir_name: &str,
) -> Result<()> {
    fs::create_dir_all(project_dir.join("refs"))?;
    fs::create_dir_all(project_dir.join("experiments"))?;
    fs::create_dir_all(project_dir.join("figures"))?;

    fs::write(project_dir.join("paper.tex"), generate_paper_tex(title))?;
    fs::write(
        project_dir.join("reproduce.yaml"),
        generate_reproduce_yaml(title, journal),
    )?;
    fs::write(
        project_dir.join("reproduce.sh"),
        "#!/usr/bin/env bash\nset -euo pipefail\nuv sync\npython experiments/run_experiment.py\n",
    )?;
    fs::write(
        project_dir.join("pyproject.toml"),
        generate_pyproject(dir_name),
    )?;
    fs::write(project_dir.join("references.bib"), "")?;
    fs::write(
        project_dir.join("refs/refs-manifest.yaml"),
        generate_refs_manifest(),
    )?;
    fs::write(
        project_dir.join("experiments/run_experiment.py"),
        "#!/usr/bin/env python3\n\"\"\"Main experiment script.\"\"\"\n\nprint(\"Experiment completed.\")\n",
    )?;
    Ok(())
}

fn generate_paper_tex(title: &str) -> String {
    format!(
        r#"\documentclass{{article}}
\title{{{title}}}
\author{{Your Name}}
\date{{\today}}

\begin{{document}}
\maketitle

\begin{{abstract}}
Your abstract here.
\end{{abstract}}

\section{{Introduction}}
Introduction text.

\section{{Methods}}
Methods description.

\section{{Results}}
Results and analysis.

\section{{Conclusion}}
Conclusion and future work.

\bibliographystyle{{plain}}
\bibliography{{references}}

\end{{document}}
"#
    )
}

fn generate_reproduce_yaml(title: &str, journal: &str) -> String {
    format!(
        r#"paper_id: ""
title: "{title}"
version: "1.0"
journal_slug: "{journal}"

runtime:
  python: "3.12"
  package_manager: "uv"

experiments:
  - name: main
    command: "python experiments/run_experiment.py"
    expected_output: "Experiment completed."

compute_tier: free
"#
    )
}

fn generate_pyproject(name: &str) -> String {
    format!(
        r#"[project]
name = "{name}"
version = "0.1.0"
requires-python = ">=3.12"
dependencies = []
"#
    )
}

fn generate_refs_manifest() -> String {
    r#"# Reference Evidence Manifest
# Upload PDFs: automl asset upload refs/ --type reference
# Fill in asset_ids below after uploading.

references: []
  # - key: author2024title
  #   asset_id: ""        # ← from: automl asset upload refs/author2024.pdf
  #   url: https://doi.org/...
"#
    .to_string()
}
