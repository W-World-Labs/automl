// ============================================================
// Auto.ml CLI — Asset Upload Command
//
// Upload files (references, datasets, figures) to R2 storage.
// Returns asset IDs for use in refs-manifest.yaml / reproduce.yaml.
// ============================================================

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

use crate::cli::{AssetCommands, OutputFormat};
use crate::client::ApiClient;
use crate::output;

/// Dispatch `automl asset <subcommand>`
pub fn handle_asset_command(
    action: AssetCommands,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    match action {
        AssetCommands::Upload { path, r#type } => {
            handle_asset_upload(&path, &r#type, format, client)
        }
        AssetCommands::List { r#type } => handle_asset_list(r#type.as_deref(), format, client),
        AssetCommands::Verify { asset_ids } => handle_asset_verify(asset_ids, format, client),
    }
}

/// Main handler for `automl asset upload <path> --type <type>`
pub fn handle_asset_upload(
    path: &str,
    asset_type: &str,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    let p = Path::new(path);
    if !p.exists() {
        anyhow::bail!("Path not found: {path}");
    }

    if p.is_dir() {
        handle_dir_upload(p, asset_type, format, client)
    } else {
        handle_single_upload(p, asset_type, format, client)
    }
}

/// Upload a single file
fn handle_single_upload(
    path: &Path,
    asset_type: &str,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Skip non-uploadable files
    if filename.starts_with('.') {
        return Ok(());
    }

    let content =
        fs::read(path).with_context(|| format!("Failed to read file: {}", path.display()))?;

    // Compute SHA256 locally for display
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let sha256 = format!("{:x}", hasher.finalize());
    let size = content.len();

    output::info(&format!(
        "Uploading {} ({}, SHA256: {}...)",
        filename,
        format_size(size),
        &sha256[..12],
    ));

    // Determine content type
    let content_type = guess_content_type(&filename);

    // Upload via multipart form
    let result = client.upload_asset(&content, &filename, &content_type, asset_type)?;

    let asset_id = result["asset_id"].as_str().unwrap_or("unknown");
    let deduped = result["deduplicated"].as_bool().unwrap_or(false);

    if deduped {
        output::success(&format!(
            "{} → {} (已存在, SHA256 匹配)",
            filename, asset_id
        ));
    } else {
        output::success(&format!(
            "{} → {} ({}, {})",
            filename,
            asset_id,
            format_size(size),
            asset_type
        ));
    }

    // If JSON format requested, print full response
    if matches!(format, OutputFormat::Json) {
        output::print_output(format, &result);
    }

    Ok(())
}

/// Upload all files in a directory
fn handle_dir_upload(
    dir: &Path,
    asset_type: &str,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    let entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory: {}", dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().map(|t| t.is_file()).unwrap_or(false)
                && !e.file_name().to_string_lossy().starts_with('.')
        })
        .collect();

    if entries.is_empty() {
        anyhow::bail!("No files found in directory: {}", dir.display());
    }

    println!();
    output::info(&format!(
        "📦 Uploading {} files from {}/ as '{}'",
        entries.len(),
        dir.display(),
        asset_type,
    ));
    println!();

    let mut uploaded = Vec::new();
    let mut errors = Vec::new();

    for entry in &entries {
        let path = entry.path();
        match handle_single_upload(&path, asset_type, format, client) {
            Ok(()) => uploaded.push(path),
            Err(e) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                output::error(&format!("{}: {}", name, e));
                errors.push(path);
            }
        }
    }

    println!();
    if errors.is_empty() {
        output::success(&format!(
            "✅ All {} files uploaded successfully",
            uploaded.len()
        ));
    } else {
        eprintln!(
            "  {} {}/{} files uploaded, {} failed",
            console::style("⚠️").yellow(),
            uploaded.len(),
            entries.len(),
            errors.len(),
        );
    }

    Ok(())
}

/// List assets for the current user
pub fn handle_asset_list(
    asset_type: Option<&str>,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    let mut params = vec![];
    if let Some(t) = asset_type {
        params.push(("type", t));
    }

    let result = client.get_with_params("/api/v1/assets", &params)?;
    let assets = result["assets"].as_array();

    if let Some(assets) = assets {
        if assets.is_empty() {
            output::info("No assets found. Upload files with: automl asset upload <file>");
            return Ok(());
        }

        if matches!(format, OutputFormat::Json) {
            output::print_output(format, &result);
            return Ok(());
        }

        // Table output
        let mut table = comfy_table::Table::new();
        table.set_header(vec![
            "Asset ID", "Filename", "Type", "Size", "SHA256", "Created",
        ]);

        for asset in assets {
            table.add_row(vec![
                asset["id"].as_str().unwrap_or("-").to_string(),
                asset["filename"].as_str().unwrap_or("-").to_string(),
                asset["asset_type"].as_str().unwrap_or("-").to_string(),
                format_size(asset["size_bytes"].as_u64().unwrap_or(0) as usize),
                asset["sha256"]
                    .as_str()
                    .map(|s| format!("{}...", &s[..12]))
                    .unwrap_or("-".to_string()),
                asset["created_at"].as_str().unwrap_or("-").to_string(),
            ]);
        }

        println!("{table}");
    } else {
        output::print_output(format, &result);
    }

    Ok(())
}

/// Verify asset IDs exist
pub fn handle_asset_verify(
    asset_ids: Vec<String>,
    format: &OutputFormat,
    client: &ApiClient,
) -> Result<()> {
    if asset_ids.is_empty() {
        anyhow::bail!("No asset IDs provided");
    }

    output::info(&format!("Verifying {} asset ID(s)...", asset_ids.len()));

    let body = serde_json::json!({ "asset_ids": asset_ids });
    let result = client.post("/api/v1/assets/verify", &body)?;

    let all_valid = result["all_valid"].as_bool().unwrap_or(false);
    let missing = result["missing"].as_array();

    if all_valid {
        output::success(&format!("✅ All {} asset IDs verified", asset_ids.len()));
    } else if let Some(missing) = missing {
        output::error(&format!("❌ {} missing asset ID(s):", missing.len()));
        for id in missing {
            eprintln!("  • {}", id.as_str().unwrap_or("unknown"));
        }
    }

    if matches!(format, OutputFormat::Json) {
        output::print_output(format, &result);
    }

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────

fn guess_content_type(filename: &str) -> String {
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "html" | "htm" | "mhtml" => "text/html",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "csv" => "text/csv",
        "json" => "application/json",
        "yaml" | "yml" => "text/yaml",
        "tex" => "text/x-tex",
        "bib" => "text/x-bibtex",
        "py" => "text/x-python",
        "sh" => "text/x-shellscript",
        "zip" => "application/zip",
        "gz" | "tar.gz" | "tgz" => "application/gzip",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
