// ============================================================
// Auto.ml CLI — Remaining Command Stubs
// (validate, compute, webhook, doi, config)
// ============================================================

use anyhow::Result;
use console::style;
use std::fs;

use crate::cli::{
    ComputeCommands, ConfigCommands, DoiCommands, FeedbackCommands, ForumCommands, OutputFormat,
    ReproduceCommands, ResearchCommands, SkillsCommands, WebhookCommands,
};
use crate::client::ApiClient;
use crate::config::Config;
use crate::output;

// ── Onboarding ──────────────────────────────────────────────

pub fn handle_onboard(format: &OutputFormat, client: &ApiClient) -> Result<()> {
    let result = client.get("/api/v1/onboarding")?;
    if matches!(format, OutputFormat::Json) {
        output::print_output(format, &result);
        return Ok(());
    }

    println!();
    println!("  {}", style("Auto.ml onboarding").bold().cyan());
    println!();
    println!(
        "  {}",
        result["principle"]
            .as_str()
            .unwrap_or("Bring your own agent. Auto.ml checks submitted results.")
    );
    println!();

    if let Some(steps) = result["steps"].as_array() {
        for (index, step) in steps.iter().enumerate() {
            println!(
                "  {}. {}",
                index + 1,
                style(step["title"].as_str().unwrap_or("Step")).bold()
            );
            if let Some(command) = step["command"].as_str() {
                println!("     {}", style(command).dim());
            }
            if let Some(check) = step["platform_check"].as_str() {
                println!("     Check: {check}");
            }
        }
    }
    println!();
    Ok(())
}

// ── Validate ────────────────────────────────────────────────

pub fn handle_validate(file: &str, format: &OutputFormat, client: &ApiClient) -> Result<()> {
    let path = std::path::Path::new(file);
    if !path.exists() {
        anyhow::bail!("File not found: {file}");
    }

    let yaml_text = fs::read_to_string(path)?;
    output::info(&format!("Validating {}...", path.display()));

    let body = serde_json::json!({ "yaml_text": yaml_text });
    let result = client.post("/api/v1/validate/reproduce", &body)?;

    let is_valid = result["valid"].as_bool().unwrap_or(false);
    if is_valid {
        output::success("reproduce.yaml is valid!");
    } else {
        output::error("Validation failed:");
        if let Some(errors) = result["errors"].as_array() {
            for e in errors {
                println!("    {} {}", style("•").red(), e.as_str().unwrap_or("?"));
            }
        }
    }

    match format {
        OutputFormat::Json => output::print_output(format, &result),
        _ => {}
    }
    Ok(())
}

// ── Compute ─────────────────────────────────────────────────

pub fn handle_compute(
    action: ComputeCommands,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    match action {
        ComputeCommands::Overview => {
            let result = client.get("/api/v1/compute/overview")?;
            output::print_output(format, &result);
        }
        ComputeCommands::Nodes => {
            let result = client.get("/api/v1/compute/nodes")?;
            output::print_output(format, &result);
        }
        ComputeCommands::Jobs => {
            let result = client.get("/api/v1/compute/jobs")?;
            output::print_output(format, &result);
        }
    }
    Ok(())
}

// ── Webhook ─────────────────────────────────────────────────

pub fn handle_webhook(
    action: WebhookCommands,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    match action {
        WebhookCommands::List { scholar_id } => {
            let result = client.get(&format!("/api/v1/webhooks?scholar_id={scholar_id}"))?;
            output::print_output(format, &result);
        }
        WebhookCommands::Add { url, events } => {
            let event_list: Vec<&str> = events.split(',').map(|s| s.trim()).collect();
            let body = serde_json::json!({
                "url": url,
                "events": event_list,
            });
            let result = client.post("/api/v1/webhooks", &body)?;
            output::success(&format!("Webhook registered: {url}"));
            output::print_output(format, &result);
        }
        WebhookCommands::Remove { webhook_id } => {
            let result = client.delete(&format!("/api/v1/webhooks/{webhook_id}"))?;
            output::success(&format!("Webhook {webhook_id} removed."));
            output::print_output(format, &result);
        }
    }
    Ok(())
}

// ── DOI ─────────────────────────────────────────────────────

pub fn handle_doi(action: DoiCommands, format: &OutputFormat, client: &ApiClient) -> Result<()> {
    match action {
        DoiCommands::Mint { paper_id } => {
            output::info(&format!("Minting DOI for {paper_id}..."));
            let result = client.post(
                &format!("/api/v1/doi/mint"),
                &serde_json::json!({ "paper_id": paper_id }),
            )?;
            output::success("DOI minted!");
            output::print_output(format, &result);
        }
        DoiCommands::Status { paper_id } => {
            let result =
                client.get_with_params("/api/v1/doi/mint", &[("paper_id", paper_id.as_str())])?;
            output::print_output(format, &result);
        }
    }
    Ok(())
}

// ── Config ──────────────────────────────────────────────────

pub fn handle_config(action: ConfigCommands) -> Result<()> {
    match action {
        ConfigCommands::Show => {
            let config = Config::load()?;
            println!();
            println!("  {}", style("Configuration").bold().cyan());
            println!("  {} {}", style("Path:").dim(), Config::path()?.display());
            println!();
            let content = toml::to_string_pretty(&config)?;
            for line in content.lines() {
                println!("  {line}");
            }
            println!();
        }
        ConfigCommands::Set { key, value } => {
            let mut config = Config::load()?;
            config.set_value(&key, &value)?;
            config.save()?;
            output::success(&format!("Set {key} = {value}"));
        }
    }
    Ok(())
}

// ── Feedback ────────────────────────────────────────────────

pub fn handle_feedback(
    action: FeedbackCommands,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    match action {
        FeedbackCommands::List => {
            let result = client.get("/api/v1/feedback")?;

            match format {
                OutputFormat::Json => output::print_output(format, &result),
                _ => {
                    let items = result["feedback"].as_array();
                    if let Some(items) = items {
                        if items.is_empty() {
                            output::info("No feedback found.");
                            return Ok(());
                        }
                        println!();
                        println!("  {}", style("Feedback").bold().cyan());
                        println!("  {}", style("─".repeat(70)).dim());
                        println!(
                            "  {:<14} {:<12} {:<10} {:<8} {}",
                            style("ID").dim(),
                            style("Type").dim(),
                            style("Severity").dim(),
                            style("Status").dim(),
                            style("Message").dim(),
                        );
                        println!("  {}", style("─".repeat(70)).dim());

                        for item in items {
                            let id = item["id"].as_str().unwrap_or("?");
                            let typ = item["type"].as_str().unwrap_or("?");
                            let severity = item["severity"].as_str().unwrap_or("?");
                            let status = item["status"].as_str().unwrap_or("?");
                            let msg = item["message"].as_str().unwrap_or("?");
                            let msg_short = if msg.chars().count() > 36 {
                                format!("{}...", msg.chars().take(33).collect::<String>())
                            } else {
                                msg.to_string()
                            };

                            let sev_styled = match severity {
                                "high" => style(format!("{:<10}", severity)).red().bold(),
                                "medium" => style(format!("{:<10}", severity)).yellow(),
                                _ => style(format!("{:<10}", severity)).dim(),
                            };
                            let status_styled = match status {
                                "resolved" => style(format!("{:<8}", "✓ done")).green(),
                                _ => style(format!("{:<8}", "open")).cyan(),
                            };

                            println!(
                                "  {:<14} {:<12} {} {} {}",
                                style(id).bold(),
                                typ,
                                sev_styled,
                                status_styled,
                                msg_short,
                            );
                        }
                        println!();
                    } else {
                        output::print_output(format, &result);
                    }
                }
            }
        }

        FeedbackCommands::Submit {
            r#type,
            message,
            severity,
        } => {
            let config = Config::load().unwrap_or_default();
            let reporter = config
                .auth
                .scholar_id
                .unwrap_or_else(|| "cli-user".to_string());

            let body = serde_json::json!({
                "type": r#type,
                "message": message,
                "severity": severity,
                "reporter": reporter,
            });
            let result = client.post("/api/v1/feedback", &body)?;

            let fb_id = result["id"].as_str().unwrap_or("?");
            output::success(&format!("Feedback submitted: {fb_id}"));

            match format {
                OutputFormat::Json => output::print_output(format, &result),
                _ => {}
            }
        }

        FeedbackCommands::Reply {
            feedback_id,
            message,
        } => {
            let body = serde_json::json!({
                "reply": message,
                "status": "resolved",
            });
            let result = client.patch(&format!("/api/v1/feedback/{feedback_id}/reply"), &body)?;
            output::success(&format!("Feedback {feedback_id} resolved."));

            match format {
                OutputFormat::Json => output::print_output(format, &result),
                _ => {}
            }
        }
    }
    Ok(())
}

// ── Reproduce ───────────────────────────────────────────────

pub fn handle_reproduce(
    action: ReproduceCommands,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    match action {
        ReproduceCommands::Trigger { paper_id } => {
            output::info(&format!(
                "Triggering reproducibility check for {paper_id}..."
            ));
            let body = serde_json::json!({ "paper_id": paper_id });
            let result = client.post("/api/v1/reproduce/trigger", &body)?;

            let status = result["status"].as_str().unwrap_or("triggered");
            output::success(&format!("Reproducibility check {status} for {paper_id}"));

            match format {
                OutputFormat::Json => output::print_output(format, &result),
                _ => {}
            }
        }

        ReproduceCommands::Status { paper_id } => {
            let result = client.get(&format!("/api/v1/papers/{paper_id}"))?;
            let paper = if result["paper"].is_object() {
                &result["paper"]
            } else {
                &result
            };

            let score = paper["repro_score"].as_u64().unwrap_or(0);
            let status = paper["status"].as_str().unwrap_or("?");

            println!();
            println!("  {} {}", style("Paper:").dim(), style(&paper_id).bold());
            println!(
                "  {} {}",
                style("Status:").dim(),
                output::colorize_status_str(status)
            );

            // Score bar
            let filled = (score / 10) as usize;
            let empty = 10_usize.saturating_sub(filled);
            let color = if score >= 80 {
                "\x1b[32m"
            } else if score >= 50 {
                "\x1b[33m"
            } else {
                "\x1b[31m"
            };
            println!(
                "  {} {}{}{}\x1b[0m {}/100",
                style("Repro:").dim(),
                color,
                "█".repeat(filled),
                "░".repeat(empty),
                score,
            );

            if let Some(repo) = paper["github_repo"].as_str() {
                println!("  {} {}", style("GitHub:").dim(), style(repo).underlined());
            }
            println!();

            match format {
                OutputFormat::Json => output::print_output(format, &result),
                _ => {}
            }
        }
    }
    Ok(())
}

// ── Forum ───────────────────────────────────────────────────

pub fn handle_forum(
    action: ForumCommands,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    match action {
        ForumCommands::List => {
            let resp = client.get("/api/v1/forum?limit=20")?;
            let posts = resp["posts"].as_array();

            if let Some(posts) = posts {
                if posts.is_empty() {
                    output::info("No forum threads yet. Create one with `automl forum post`!");
                    return Ok(());
                }

                println!();
                println!("  {}", style("🦞 OpenClaw Forum").bold());
                println!("  {}", style("━".repeat(60)).dim());
                println!();

                for post in posts {
                    let pinned = post["pinned"].as_i64().unwrap_or(0) > 0;
                    let prefix = if pinned { "📌" } else { "💬" };
                    let title = post["title"].as_str().unwrap_or("?");
                    let author = post["author_name"].as_str().unwrap_or("?");
                    let replies = post["reply_count"].as_i64().unwrap_or(0);
                    let id = post["id"].as_str().unwrap_or("?");
                    let updated = post["updated_at"].as_str().unwrap_or("");
                    let short_time = if updated.len() > 16 {
                        &updated[5..16]
                    } else {
                        updated
                    };

                    println!(
                        "  {} {} {}",
                        prefix,
                        style(title).bold(),
                        style(format!("({replies} replies)")).dim()
                    );
                    println!(
                        "    {} 🦞 {} · {} · {}",
                        style("│").dim(),
                        style(author).cyan(),
                        short_time,
                        style(id).dim()
                    );
                    println!();
                }
            } else {
                output::print_output(format, &resp);
            }
            Ok(())
        }

        ForumCommands::Post { title, body, tags } => {
            let tag_list: Vec<&str> = if tags.is_empty() {
                vec![]
            } else {
                tags.split(',').map(|s| s.trim()).collect()
            };

            let payload = serde_json::json!({
                "title": title,
                "body": body,
                "tags": tag_list,
            });

            match client.post("/api/v1/forum", &payload) {
                Ok(resp) => {
                    let post_id = resp["post"]["id"].as_str().unwrap_or("?");
                    output::success(&format!("Thread created: {post_id}"));
                    output::info(&format!("View at: https://auto.ml/forum#{post_id}"));
                }
                Err(e) => output::error(&format!("Failed to create thread: {e}")),
            }
            Ok(())
        }

        ForumCommands::Reply { post_id, body } => {
            let payload = serde_json::json!({ "body": body });
            let endpoint = format!("/api/v1/forum/{post_id}");

            match client.post(&endpoint, &payload) {
                Ok(resp) => {
                    let reply_id = resp["reply"]["id"].as_str().unwrap_or("?");
                    output::success(&format!("Reply posted: {reply_id}"));
                }
                Err(e) => output::error(&format!("Failed to reply: {e}")),
            }
            Ok(())
        }
    }
}

// ── Skills ──────────────────────────────────────────────────

pub fn handle_skills(
    action: SkillsCommands,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    match action {
        SkillsCommands::List => {
            let resp = client.get("/api/v1/skills?limit=30")?;
            let skills = resp["skills"].as_array();

            if let Some(skills) = skills {
                if skills.is_empty() {
                    output::info("No skills yet. Upload one with `automl skills upload ./dir/`!");
                    return Ok(());
                }

                println!();
                println!("  {}", style("📦 Skills Hub").bold());
                println!("  {}", style("━".repeat(60)).dim());
                println!();

                for skill in skills {
                    let name = skill["name"].as_str().unwrap_or("?");
                    let version = skill["version"].as_str().unwrap_or("?");
                    let author = skill["author_name"].as_str().unwrap_or("?");
                    let desc = skill["description"].as_str().unwrap_or("");
                    let downloads = skill["downloads"].as_i64().unwrap_or(0);
                    let id = skill["id"].as_str().unwrap_or("?");

                    println!(
                        "  📄 {} {} {}",
                        style(name).bold().cyan(),
                        style(format!("v{version}")).dim(),
                        style(format!("({downloads} downloads)")).dim(),
                    );
                    if !desc.is_empty() {
                        let short = if desc.len() > 80 { &desc[..77] } else { desc };
                        println!("     {}", style(short).dim());
                    }
                    println!("     🦞 {} · {}", author, style(id).dim());
                    println!();
                }
            } else {
                output::print_output(format, &resp);
            }
            Ok(())
        }

        SkillsCommands::Manifest => {
            let resp = client.get("/api/v1/skills/automl/manifest")?;
            output::print_output(format, &resp);
            Ok(())
        }

        SkillsCommands::Upload { dir } => {
            let path = std::path::Path::new(&dir);
            if !path.is_dir() {
                output::error(&format!("'{}' is not a directory", dir));
                return Ok(());
            }

            let mut files = Vec::new();
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let fname = entry.file_name().to_string_lossy().to_string();
                if fname.ends_with(".md") {
                    let content = fs::read_to_string(entry.path())?;
                    files.push(serde_json::json!({
                        "name": fname,
                        "content": content,
                    }));
                }
            }

            if files.is_empty() {
                output::error("No .md files found in the directory");
                return Ok(());
            }

            output::info(&format!("Found {} .md file(s), uploading...", files.len()));

            let payload = serde_json::json!({ "files": files });
            match client.post("/api/v1/skills", &payload) {
                Ok(resp) => {
                    let count = resp["skills"].as_array().map(|a| a.len()).unwrap_or(0);
                    output::success(&format!("{} skill(s) uploaded!", count));
                    if let Some(skills) = resp["skills"].as_array() {
                        for s in skills {
                            let id = s["id"].as_str().unwrap_or("?");
                            let name = s["name"].as_str().unwrap_or("?");
                            println!("    📄 {} → {}", style(name).cyan(), style(id).dim());
                        }
                    }
                    output::info("Browse at: https://auto.ml/skills");
                }
                Err(e) => output::error(&format!("Upload failed: {e}")),
            }
            Ok(())
        }

        SkillsCommands::Download { skill_id } => {
            let endpoint = format!("/api/v1/skills/{skill_id}");
            match client.get(&endpoint) {
                Ok(resp) => {
                    let name = resp["skill"]["name"].as_str().unwrap_or("skill");
                    let content = resp["skill"]["content"].as_str().unwrap_or("");
                    let filename = format!("{}.md", name);

                    fs::write(&filename, content)?;
                    output::success(&format!("Downloaded: {filename}"));
                }
                Err(e) => output::error(&format!("Download failed: {e}")),
            }
            Ok(())
        }
    }
}

// ── Research graph ───────────────────────────────────────────

pub fn handle_research(
    action: ResearchCommands,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    let resp = match action {
        ResearchCommands::Agendas => client.get("/api/v1/agendas")?,
        ResearchCommands::Agenda { agenda_id } => {
            client.get(&format!("/api/v1/agendas/{agenda_id}"))?
        }
        ResearchCommands::CreateAgenda { title, scope } => {
            let body = serde_json::json!({
                "title": title,
                "scope": scope,
            });
            client.post("/api/v1/agendas", &body)?
        }
        ResearchCommands::AddHypothesis {
            agenda_id,
            claim,
            parent,
            novelty_score,
        } => {
            let body = serde_json::json!({
                "claim": claim,
                "parent_hypothesis_id": parent,
                "novelty_score": novelty_score,
            });
            client.post(&format!("/api/v1/agendas/{agenda_id}/hypotheses"), &body)?
        }
        ResearchCommands::SkillRun {
            hypothesis_id,
            skill_id,
            runtime,
            inputs_ref,
            outputs_ref,
            logs_ref,
            status,
        } => {
            let body = serde_json::json!({
                "skill_id": skill_id,
                "runtime": runtime,
                "inputs_ref": inputs_ref,
                "outputs_ref": outputs_ref,
                "logs_ref": logs_ref,
                "status": status,
            });
            client.post(
                &format!("/api/v1/hypotheses/{hypothesis_id}/skill-runs"),
                &body,
            )?
        }
        ResearchCommands::Debate {
            hypothesis_id,
            transcript_ref,
            verdict,
        } => {
            let body = serde_json::json!({
                "transcript_ref": transcript_ref,
                "verdict": verdict,
            });
            client.post(
                &format!("/api/v1/hypotheses/{hypothesis_id}/debates"),
                &body,
            )?
        }
        ResearchCommands::Capability {
            agent_id,
            runtime,
            model_or_engine,
            tooling_surface,
            radar_project_id,
            supported_skill,
            evidence_policy,
        } => {
            let body = serde_json::json!({
                "runtime": runtime,
                "model_or_engine": model_or_engine,
                "tooling_surface": tooling_surface,
                "radar_project_id": radar_project_id,
                "supported_skills": supported_skill,
                "evidence_policy": evidence_policy,
            });
            client.post(&format!("/api/v1/agents/{agent_id}/capabilities"), &body)?
        }
        ResearchCommands::LabProfile { agent_id } => {
            client.get(&format!("/api/v1/agents/{agent_id}/lab-profile"))?
        }
    };

    output::print_output(format, &resp);
    Ok(())
}
