// ============================================================
// Auto.ml CLI — Smart Submit Command
// ============================================================

use anyhow::{Context, Result};
use base64::Engine as _;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::Path;

use crate::cli::OutputFormat;
use crate::client::ApiClient;
use crate::config::Config;
use crate::output;

pub fn handle(
    dir: &str,
    journal: Option<String>,
    dry_run: bool,
    yes: bool,
    display_name: Option<String>,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    let project_dir = Path::new(dir);
    if !project_dir.is_dir() {
        anyhow::bail!("'{}' is not a directory", dir);
    }

    println!();
    println!("  {}", style("📦 Scanning project...").bold());
    println!();

    // ── Auto-detect project files ──────────────────────────
    let paper_tex = project_dir.join("paper.tex");
    let paper_md = project_dir.join("paper.md");
    let reproduce_yaml = project_dir.join("reproduce.yaml");
    let pyproject = project_dir.join("pyproject.toml");
    let uv_lock = project_dir.join("uv.lock");
    let references_bib = project_dir.join("references.bib");
    let refs_dir = project_dir.join("refs");
    let reproduce_sh = project_dir.join("reproduce.sh");

    // Collect ALL validation errors (don't fail-fast — let AI fix all at once)
    let mut errors: Vec<(String, String)> = Vec::new(); // (error, fix hint)

    // Paper source
    let paper_file = if paper_tex.exists() {
        output::success(&format!("paper.tex found ({})", file_size(&paper_tex)?));
        Some(paper_tex.clone())
    } else if paper_md.exists() {
        output::success(&format!("paper.md found ({})", file_size(&paper_md)?));
        Some(paper_md.clone())
    } else {
        errors.push((
            "No paper.tex or paper.md found".to_string(),
            "Create paper.tex (LaTeX) or paper.md (Markdown) with your paper content.".to_string(),
        ));
        None
    };

    // reproduce.yaml existence + content validation
    let has_reproduce = reproduce_yaml.exists();
    let mut yaml_has_experiments = false;
    let mut yaml_has_runtime = false;
    let mut yaml_paper_type = String::from("research");

    if has_reproduce {
        output::success("reproduce.yaml found");
        let yaml_content = fs::read_to_string(&reproduce_yaml).unwrap_or_default();

        // Content validation: check key fields
        let mut has_title = false;
        for line in yaml_content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("title:") {
                let val = trimmed
                    .trim_start_matches("title:")
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'');
                if !val.is_empty() && val != "Untitled" && val != "Untitled Paper" {
                    has_title = true;
                }
            }
            if trimmed.starts_with("experiments:") || trimmed == "experiments:" {
                yaml_has_experiments = true;
            }
            if trimmed.starts_with("runtime:") || trimmed == "runtime:" {
                yaml_has_runtime = true;
            }
            if trimmed.starts_with("paper_type:") {
                yaml_paper_type = trimmed
                    .trim_start_matches("paper_type:")
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
            }
        }

        if !has_title {
            errors.push((
                "reproduce.yaml: missing or empty 'title' field".to_string(),
                "Add `title: \"Your Paper Title\"` to reproduce.yaml.".to_string(),
            ));
        }

        // Only require experiments for research papers (not surveys)
        if yaml_paper_type == "research" {
            if !yaml_has_experiments {
                errors.push((
                    "reproduce.yaml: no 'experiments' section defined".to_string(),
                    "Add at least one experiment block. See genesis template:\n       experiments:\n         - name: \"main\"\n           command: \"uv run python experiments/run.py\"\n           seed: 42".to_string(),
                ));
            }
            if !yaml_has_runtime {
                errors.push((
                    "reproduce.yaml: no 'runtime' section defined".to_string(),
                    "Add runtime config:\n       runtime:\n         python: \"3.12\"\n         package_manager: \"uv\"\n         bootstrap: \"uv sync\"".to_string(),
                ));
            }
        }
    } else {
        errors.push((
            "reproduce.yaml not found".to_string(),
            "Run `automl init` to scaffold a project with reproduce.yaml,\n       or create one manually: https://auto.ml/docs#reproduce".to_string(),
        ));
    }

    // Python deps
    if pyproject.exists() {
        let lock_status = if uv_lock.exists() {
            "+ uv.lock"
        } else {
            "(no uv.lock)"
        };
        output::success(&format!("pyproject.toml {lock_status} found"));
    } else if yaml_has_experiments {
        // Only warn about missing pyproject if experiments exist (implies code)
        errors.push((
            "pyproject.toml not found (needed for Python experiments)".to_string(),
            "Run `uv init` to create pyproject.toml with your dependencies.".to_string(),
        ));
    }

    // References
    if references_bib.exists() {
        let bib_content = fs::read_to_string(&references_bib).unwrap_or_default();
        let entry_count = bib_content.matches("@").count();
        output::success(&format!("references.bib ({} entries)", entry_count));

        if refs_dir.exists() {
            let ref_files: Vec<_> = fs::read_dir(&refs_dir)?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .collect();
            output::success(&format!(
                "refs/ directory ({}/{} evidence files)",
                ref_files.len(),
                entry_count
            ));
        }
    }

    // reproduce.sh
    if reproduce_sh.exists() {
        output::success("reproduce.sh found");
    }

    // figures/ directory
    let figures_dir = project_dir.join("figures");
    if figures_dir.exists() && figures_dir.is_dir() {
        let img_count = fs::read_dir(&figures_dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| {
                        let ext = e
                            .path()
                            .extension()
                            .and_then(|x| x.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        ["png", "jpg", "jpeg", "pdf", "svg", "eps", "gif"].contains(&ext.as_str())
                    })
                    .count()
            })
            .unwrap_or(0);
        if img_count > 0 {
            output::success(&format!("figures/ ({} images)", img_count));
        }
    }

    // ── Validate \includegraphics references ────────────────
    if let Some(ref pf) = paper_file {
        if let Ok(tex_content) = fs::read_to_string(pf) {
            let mut missing_imgs: Vec<String> = Vec::new();
            for line in tex_content.lines() {
                if let Some(start) = line.find("\\includegraphics") {
                    let rest = &line[start..];
                    if let Some(open) = rest.find('{') {
                        if let Some(close) = rest[open..].find('}') {
                            let img_ref = &rest[open + 1..open + close];
                            let img_path = project_dir.join(img_ref);
                            let exists = img_path.exists()
                                || project_dir.join(format!("{img_ref}.png")).exists()
                                || project_dir.join(format!("{img_ref}.jpg")).exists()
                                || project_dir.join(format!("{img_ref}.pdf")).exists();
                            if !exists {
                                missing_imgs.push(img_ref.to_string());
                            }
                        }
                    }
                }
            }
            if !missing_imgs.is_empty() {
                errors.push((
                    format!("{} image reference(s) not found: {}", missing_imgs.len(), missing_imgs.join(", ")),
                    "Add the missing image files to your project directory, or fix the \\includegraphics paths.".to_string(),
                ));
            }
        }
    }

    // ── Parse reproduce.yaml for metadata ──────────────────
    let mut title = String::from("Untitled Paper");
    let mut journal_slug = String::new();

    if has_reproduce {
        let yaml_content = fs::read_to_string(&reproduce_yaml)?;
        for line in yaml_content.lines() {
            let line = line.trim();
            if line.starts_with("title:") {
                title = line
                    .trim_start_matches("title:")
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
            }
            if line.starts_with("journal_slug:") {
                journal_slug = line
                    .trim_start_matches("journal_slug:")
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
            }
        }
    }

    // Fallback: extract \title{} from LaTeX if title is still default
    if title == "Untitled Paper" {
        if let Some(ref pf) = paper_file {
            if let Ok(tex_source) = fs::read_to_string(pf) {
                if let Some(extracted) = extract_title(&tex_source) {
                    title = extracted;
                }
            }
        }
    }

    // Resolve journal
    let config = Config::load()?;
    let final_journal = journal.unwrap_or_else(|| {
        if !journal_slug.is_empty() {
            journal_slug.clone()
        } else {
            config.defaults.journal.clone()
        }
    });

    // ── Submission summary ─────────────────────────────────
    println!();
    println!("  {}", style("📋 Submission Summary:").bold());
    println!("  {} {}", style("Title:").dim(), title);
    println!("  {} {}", style("Journal:").dim(), &final_journal);
    if let Some(ref dn) = display_name {
        println!(
            "  {} {} {}",
            style("Author:").dim(),
            dn,
            style("(pseudonym)").yellow()
        );
    }
    println!();

    // ── Report ALL errors at once ──────────────────────────
    if !errors.is_empty() {
        println!(
            "  {}",
            style(format!("✗ {} issue(s) found:", errors.len()))
                .red()
                .bold()
        );
        println!();
        for (i, (err, fix)) in errors.iter().enumerate() {
            println!("    {}. {}", style(i + 1).red().bold(), style(err).red());
            for fix_line in fix.lines() {
                println!("       {} {}", style("Fix:").cyan(), fix_line);
            }
            println!();
        }
        anyhow::bail!(
            "Cannot submit: {} validation issue(s) must be resolved.\n\
             All issues are listed above with fix instructions.",
            errors.len()
        );
    }

    if dry_run {
        output::info("Dry run mode — no submission made.");
        return Ok(());
    }

    // ── Confirm ────────────────────────────────────────────
    if !yes {
        let confirm = dialoguer::Confirm::new()
            .with_prompt("  Confirm submission?")
            .default(true)
            .interact()?;

        if !confirm {
            output::info("Submission cancelled.");
            return Ok(());
        }
    }

    // ── Submit ─────────────────────────────────────────────
    println!();
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈")
            .template("  {spinner:.cyan} {msg}")?,
    );
    pb.set_message("Submitting to auto.ml...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    // Read paper content for abstract extraction
    let paper_content = fs::read_to_string(paper_file.as_ref().unwrap()).unwrap_or_default();
    let abstract_text = extract_abstract(&paper_content);

    // Build authors — always include at least one author from config or display_name
    let authors = if let Some(ref dn) = display_name {
        serde_json::json!([{
            "display_name": dn,
            "name": dn,
            "scholar_id": config.auth.scholar_id.as_deref().unwrap_or(""),
        }])
    } else {
        // Auto-populate from config
        let scholar_id = config.auth.scholar_id.clone().unwrap_or_default();
        let name = if !scholar_id.is_empty() {
            scholar_id.clone()
        } else {
            gethostname::gethostname().to_string_lossy().to_string()
        };
        serde_json::json!([{
            "name": name,
            "scholar_id": scholar_id,
            "type": "agent",
        }])
    };

    let body = serde_json::json!({
        "title": title,
        "abstract": abstract_text,
        "domain": "Machine Learning",
        "journal_slug": final_journal,
        "paper_type": "research",
        "compute_tier": config.defaults.compute_tier,
        "authors": authors,
    });

    let result = client.post("/api/v1/papers", &body);
    pb.finish_and_clear();

    match result {
        Ok(resp) => {
            let paper_id = resp["paper_id"].as_str().unwrap_or("?");
            let status = resp["status"].as_str().unwrap_or("SUBMITTED");
            println!();
            output::success(&format!("Paper created: {paper_id}"));
            output::success(&format!("Status: {status}"));

            // ── Upload project files to R2 ─────────────────────
            println!();
            let upload_pb = ProgressBar::new_spinner();
            upload_pb.set_style(
                ProgressStyle::default_spinner()
                    .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈")
                    .template("  {spinner:.cyan} {msg}")?,
            );
            upload_pb.set_message("Uploading project files...");
            upload_pb.enable_steady_tick(std::time::Duration::from_millis(100));

            let files = collect_project_files(project_dir)?;
            if !files.is_empty() {
                let upload_body = serde_json::json!({ "files": files });
                let upload_endpoint = format!("/api/v1/papers/{}/files", paper_id);
                match client.post(&upload_endpoint, &upload_body) {
                    Ok(upload_resp) => {
                        upload_pb.finish_and_clear();
                        let count = upload_resp["file_count"].as_u64().unwrap_or(0);
                        let size = upload_resp["total_size"].as_u64().unwrap_or(0);
                        output::success(&format!(
                            "Uploaded {} files ({:.1}KB) to paper package",
                            count,
                            size as f64 / 1024.0
                        ));
                    }
                    Err(e) => {
                        upload_pb.finish_and_clear();
                        output::error(&format!("File upload failed: {e}"));
                        output::info("Paper was created but files not uploaded. Use `automl upload` to retry.");
                    }
                }
            } else {
                upload_pb.finish_and_clear();
                output::info("No additional files found to upload.");
            }

            println!();
            output::info(&format!("Track progress: automl status {paper_id} --watch"));

            match format {
                OutputFormat::Json => output::print_output(format, &resp),
                OutputFormat::Minimal => println!("{paper_id}"),
                _ => {}
            }
        }
        Err(e) => {
            output::error(&format!("Submission failed: {e}"));
            output::info("Run `automl validate` to check your reproduce.yaml");
        }
    }
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

fn extract_abstract(content: &str) -> String {
    // LaTeX abstract extraction
    if let Some(start) = content.find("\\begin{abstract}") {
        if let Some(end) = content.find("\\end{abstract}") {
            let inner = &content[start + 16..end];
            return inner.trim().to_string();
        }
    }
    // Markdown abstract (after "## Abstract" heading)
    for (i, line) in content.lines().enumerate() {
        if line.trim().eq_ignore_ascii_case("## abstract")
            || line.trim().eq_ignore_ascii_case("# abstract")
        {
            let abstract_lines: Vec<&str> = content
                .lines()
                .skip(i + 1)
                .take_while(|l| !l.starts_with('#'))
                .collect();
            return abstract_lines.join("\n").trim().to_string();
        }
    }
    String::new()
}

/// Collect all project files for upload to R2
/// Supports both text files (inline) and binary files (base64-encoded)
fn collect_project_files(project_dir: &Path) -> Result<Vec<serde_json::Value>> {
    let mut files = Vec::new();

    // Text file extensions (uploaded as-is)
    let text_extensions = [
        "tex", "md", "yaml", "yml", "bib", "py", "sh", "rs", "json", "toml", "txt", "cfg", "ini",
        "csv", "r", "jl",
    ];

    // Binary file extensions (uploaded as base64)
    let binary_extensions = [
        "png", "jpg", "jpeg", "pdf", "svg", "eps", "gif", "tiff", "webp",
    ];

    // Walk directory recursively
    fn walk_dir(
        dir: &Path,
        base: &Path,
        files: &mut Vec<serde_json::Value>,
        text_extensions: &[&str],
        binary_extensions: &[&str],
    ) -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden dirs, build artifacts, large dirs
            if name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "__pycache__"
                || name == ".git"
                || name == "venv"
                || name == ".venv"
            {
                continue;
            }

            if path.is_dir() {
                walk_dir(&path, base, files, text_extensions, binary_extensions)?;
            } else if path.is_file() {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                let relative = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();

                if text_extensions.contains(&ext.as_str()) {
                    // Text files: skip if > 1MB
                    let meta = fs::metadata(&path)?;
                    if meta.len() > 1_000_000 {
                        continue;
                    }

                    if let Ok(content) = fs::read_to_string(&path) {
                        files.push(serde_json::json!({
                            "filename": relative,
                            "content": content,
                        }));
                    }
                } else if binary_extensions.contains(&ext.as_str()) {
                    // Binary files: base64-encode, skip if > 10MB
                    let meta = fs::metadata(&path)?;
                    if meta.len() > 10_000_000 {
                        continue;
                    }

                    if let Ok(bytes) = fs::read(&path) {
                        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        files.push(serde_json::json!({
                            "filename": relative,
                            "content": encoded,
                            "encoding": "base64",
                        }));
                    }
                }
            }
        }
        Ok(())
    }

    walk_dir(
        project_dir,
        project_dir,
        &mut files,
        &text_extensions,
        &binary_extensions,
    )?;

    Ok(files)
}

/// Extract \title{...} from LaTeX source
fn extract_title(content: &str) -> Option<String> {
    // Find \title{...} — may span multiple lines
    let marker = "\\title{";
    if let Some(start) = content.find(marker) {
        let rest = &content[start + marker.len()..];
        // Count braces to find matching close
        let mut depth = 1;
        let mut end = 0;
        for (i, ch) in rest.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        if end > 0 {
            let raw = &rest[..end];
            // Clean up: collapse whitespace, strip common LaTeX commands
            let cleaned = raw
                .replace("\\\\", " ")
                .replace("\\textbf{", "")
                .replace("\\textit{", "")
                .replace("\\emph{", "")
                .replace("}", "")
                .replace('\n', " ");
            let title: String = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
            if !title.is_empty() {
                return Some(title);
            }
        }
    }
    None
}
