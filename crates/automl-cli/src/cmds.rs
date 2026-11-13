//! papers / runs / keys command implementations + self-update.

use serde_json::{json, Value};

use crate::{
    client::Api,
    output::{print_list, print_value, short_id, Format},
};

pub fn papers_list(api: &Api, fmt: &Format, limit: u32, all: bool) -> anyhow::Result<()> {
    let items = api.get_paged("/api/v1/papers", limit, all)?;
    print_list(
        fmt,
        &items,
        &["ID", "TITLE", "REVIEW", "CODE", "CREATED"],
        |p| {
            vec![
                short_id(&p["id"]),
                p["title"].as_str().unwrap_or("").to_string(),
                p["review_status"].as_str().unwrap_or("?").to_string(),
                p["code_status"].as_str().unwrap_or("?").to_string(),
                p["created_at"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(10)
                    .collect(),
            ]
        },
    );
    Ok(())
}

pub fn papers_get(api: &Api, fmt: &Format, id: &str) -> anyhow::Result<()> {
    let p = api.get(&format!("/api/v1/papers/{id}"))?;
    print_value(fmt, &p, |p| {
        format!(
            "{}\n  review: {}   code: {}\n  repo:   {} @ {}\n  spec:   {}",
            p["title"].as_str().unwrap_or("?"),
            p["review_status"].as_str().unwrap_or("?"),
            p["code_status"].as_str().unwrap_or("?"),
            p["repo_url"].as_str().unwrap_or("-"),
            p["commit_sha"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("(unpinned)"),
            p["reproduce_path"].as_str().unwrap_or("reproduce.yaml"),
        )
    });
    Ok(())
}

/// Submit a draft paper for review (`draft → submitted`).
pub fn papers_submit(api: &Api, fmt: &Format, id: &str) -> anyhow::Result<()> {
    api.require_key()?;
    let p = api.patch(
        &format!("/api/v1/papers/{id}"),
        &json!({"review_status": "submitted"}),
    )?;
    print_value(fmt, &p, |p| {
        format!(
            "paper {} → {}",
            short_id(&p["id"]),
            p["review_status"].as_str().unwrap_or("?")
        )
    });
    Ok(())
}

// ── Research Hub projects (P0) ──────────────────────────────────────────────

pub fn project_create(
    api: &Api,
    fmt: &Format,
    slug: Option<String>,
    title: String,
    summary: String,
    visibility: String,
) -> anyhow::Result<()> {
    api.require_key()?;
    let mut body = json!({
        "title": title,
        "summary": summary,
        "visibility": visibility,
    });
    if let Some(s) = slug {
        body["slug"] = json!(s);
    }
    let p = api.post("/api/v1/research-projects", &body)?;
    print_value(fmt, &p, |p| {
        format!(
            "project {} created — {} ({} / {})",
            short_id(&p["id"]),
            p["slug"].as_str().unwrap_or("?"),
            p["status"].as_str().unwrap_or("?"),
            p["visibility"].as_str().unwrap_or("?"),
        )
    });
    Ok(())
}

pub fn project_list(api: &Api, fmt: &Format, limit: u32, all: bool) -> anyhow::Result<()> {
    api.require_key()?;
    let items = api.get_paged("/api/v1/research-projects", limit, all)?;
    print_list(
        fmt,
        &items,
        &["ID", "SLUG", "TITLE", "STATUS", "VIS", "CREATED"],
        |p| {
            vec![
                short_id(&p["id"]),
                p["slug"].as_str().unwrap_or("").to_string(),
                p["title"].as_str().unwrap_or("").to_string(),
                p["status"].as_str().unwrap_or("?").to_string(),
                p["visibility"].as_str().unwrap_or("?").to_string(),
                p["created_at"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(10)
                    .collect(),
            ]
        },
    );
    Ok(())
}

pub fn project_get(api: &Api, fmt: &Format, id: &str) -> anyhow::Result<()> {
    api.require_key()?;
    let p = api.get(&format!("/api/v1/research-projects/{id}"))?;
    print_value(fmt, &p, |p| {
        format!(
            "{} ({})\n  status: {}   visibility: {}\n  summary: {}\n  daas_workspace: {}",
            p["title"].as_str().unwrap_or("?"),
            p["slug"].as_str().unwrap_or("?"),
            p["status"].as_str().unwrap_or("?"),
            p["visibility"].as_str().unwrap_or("?"),
            p["summary"].as_str().unwrap_or(""),
            p["w_daas_workspace_id"].as_str().unwrap_or("-"),
        )
    });
    Ok(())
}

pub fn project_member_add(
    api: &Api,
    fmt: &Format,
    project_id: &str,
    scholar: Option<String>,
    agent: Option<String>,
    role: &str,
) -> anyhow::Result<()> {
    api.require_key()?;
    if scholar.is_some() == agent.is_some() {
        anyhow::bail!("pass exactly one of --scholar or --agent");
    }
    let mut body = json!({"role": role});
    if let Some(s) = scholar {
        body["scholar"] = json!(s);
    }
    if let Some(a) = agent {
        body["agent"] = json!(a);
    }
    let m = api.post(
        &format!("/api/v1/research-projects/{project_id}/members"),
        &body,
    )?;
    print_value(fmt, &m, |m| {
        let who = m["scholar_w_id"]
            .as_str()
            .map(str::to_string)
            .or_else(|| m["agent_name"].as_str().map(str::to_string))
            .unwrap_or_else(|| short_id(&m["id"]));
        format!(
            "{} added as {} to project {}",
            who,
            m["role"].as_str().unwrap_or("?"),
            short_id(&m["project_id"]),
        )
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn project_paper_add(
    api: &Api,
    fmt: &Format,
    project_id: &str,
    doi: Option<String>,
    arxiv_id: Option<String>,
    w_papers_work_id: Option<String>,
    openalex_id: Option<String>,
    daas_document_id: Option<String>,
    fuel_role: Option<&str>,
    notes: Option<String>,
    tags: Vec<String>,
) -> anyhow::Result<()> {
    api.require_key()?;
    let identifiers = [
        ("doi", doi),
        ("arxiv_id", arxiv_id),
        ("w_papers_work_id", w_papers_work_id),
        ("openalex_id", openalex_id),
        ("daas_document_id", daas_document_id),
    ];
    let mut body = json!({});
    let mut count = 0;
    for (name, value) in identifiers {
        if let Some(value) = value {
            body[name] = json!(value);
            count += 1;
        }
    }
    if count != 1 {
        anyhow::bail!("pass exactly one primary paper identifier");
    }
    if let Some(role) = fuel_role {
        body["fuel_role"] = json!(role);
    }
    if let Some(notes) = notes {
        body["notes"] = json!(notes);
    }
    if !tags.is_empty() {
        body["tags"] = json!(tags);
    }
    let paper = api.post(
        &format!("/api/v1/research-projects/{project_id}/papers"),
        &body,
    )?;
    print_value(fmt, &paper, |paper| {
        format!(
            "paper {} added to project {} ({})",
            paper["cached_title"]
                .as_str()
                .or_else(|| paper["doi"].as_str())
                .or_else(|| paper["arxiv_id"].as_str())
                .or_else(|| paper["w_papers_work_id"].as_str())
                .or_else(|| paper["openalex_id"].as_str())
                .or_else(|| paper["daas_document_id"].as_str())
                .unwrap_or("paper"),
            short_id(&paper["project_id"]),
            paper["fuel_role"].as_str().unwrap_or("seed"),
        )
    });
    Ok(())
}

// ── Research Hub P2: DaaS binding, assets, search ──────────────────────────

pub fn project_bind_daas(
    api: &Api,
    fmt: &Format,
    project_id: &str,
    workspace_id: &str,
) -> anyhow::Result<()> {
    api.require_key()?;
    let b = api.send(
        reqwest::Method::PUT,
        &format!("/api/v1/research-projects/{project_id}/daas/binding"),
        Some(&json!({"workspace_id": workspace_id})),
    )?;
    print_value(fmt, &b, |b| {
        format!(
            "project {} bound to workspace {} (provider: {}, verified: {})",
            short_id(&b["project_id"]),
            b["workspace_id"].as_str().unwrap_or("?"),
            b["provider_name"].as_str().unwrap_or("?"),
            b["verified"].as_bool().unwrap_or(false),
        )
    });
    Ok(())
}

pub fn project_unbind_daas(api: &Api, fmt: &Format, project_id: &str) -> anyhow::Result<()> {
    api.require_key()?;
    api.delete(&format!(
        "/api/v1/research-projects/{project_id}/daas/binding"
    ))?;
    print_value(
        fmt,
        &json!({"status": "unbound", "project_id": project_id}),
        |_| format!("project {project_id} unbound"),
    );
    Ok(())
}

pub fn project_binding(api: &Api, fmt: &Format, project_id: &str) -> anyhow::Result<()> {
    api.require_key()?;
    let b = api.get(&format!(
        "/api/v1/research-projects/{project_id}/daas/binding"
    ))?;
    print_value(fmt, &b, |b| {
        format!(
            "workspace {} ({}, verified: {}, bound {})",
            b["workspace_id"].as_str().unwrap_or("?"),
            b["provider"].as_str().unwrap_or("?"),
            b["verified"].as_bool().unwrap_or(false),
            b["created_at"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(10)
                .collect::<String>(),
        )
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn project_asset_add(
    api: &Api,
    fmt: &Format,
    project_id: &str,
    kind: &str,
    title: Option<String>,
    uri: Option<String>,
    daas_artifact_id: Option<String>,
    metadata: Option<String>,
) -> anyhow::Result<()> {
    api.require_key()?;
    let mut body = json!({"kind": kind});
    if let Some(t) = title {
        body["title"] = json!(t);
    }
    if let Some(u) = uri {
        body["uri"] = json!(u);
    }
    if let Some(d) = daas_artifact_id {
        body["daas_artifact_id"] = json!(d);
    }
    if let Some(m) = metadata {
        let parsed: Value = serde_json::from_str(&m)
            .map_err(|e| anyhow::anyhow!("--metadata must be valid JSON: {e}"))?;
        if !parsed.is_object() {
            anyhow::bail!("--metadata must be a JSON object");
        }
        body["metadata"] = parsed;
    }
    let asset = api.post(
        &format!("/api/v1/research-projects/{project_id}/assets"),
        &body,
    )?;
    print_value(fmt, &asset, |asset| {
        format!(
            "asset {} registered on project {} ({})",
            short_id(&asset["id"]),
            short_id(&asset["project_id"]),
            asset["kind"].as_str().unwrap_or("?"),
        )
    });
    Ok(())
}

pub fn project_assets(
    api: &Api,
    fmt: &Format,
    project_id: &str,
    kind: Option<&str>,
    limit: u32,
    all: bool,
) -> anyhow::Result<()> {
    api.require_key()?;
    let mut path = format!("/api/v1/research-projects/{project_id}/assets");
    if let Some(k) = kind {
        path.push_str(&format!("?kind={k}"));
    }
    let items = api.get_paged(&path, limit, all)?;
    print_list(
        fmt,
        &items,
        &["ID", "KIND", "TITLE", "POINTER", "CREATED"],
        |a| {
            vec![
                short_id(&a["id"]),
                a["kind"].as_str().unwrap_or("?").to_string(),
                a["title"].as_str().unwrap_or("").to_string(),
                a["uri"]
                    .as_str()
                    .or_else(|| a["daas_artifact_id"].as_str())
                    .unwrap_or("-")
                    .to_string(),
                a["created_at"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(10)
                    .collect(),
            ]
        },
    );
    Ok(())
}

pub fn project_asset_get(
    api: &Api,
    fmt: &Format,
    project_id: &str,
    asset_id: &str,
) -> anyhow::Result<()> {
    api.require_key()?;
    let a = api.get(&format!(
        "/api/v1/research-projects/{project_id}/assets/{asset_id}"
    ))?;
    print_value(fmt, &a, |a| {
        format!(
            "asset {} ({}) — {}\n  uri: {}\n  daas_artifact: {}",
            short_id(&a["id"]),
            a["kind"].as_str().unwrap_or("?"),
            a["title"].as_str().unwrap_or(""),
            a["uri"].as_str().unwrap_or("-"),
            a["daas_artifact_id"].as_str().unwrap_or("-"),
        )
    });
    Ok(())
}

pub fn project_asset_rm(
    api: &Api,
    fmt: &Format,
    project_id: &str,
    asset_id: &str,
) -> anyhow::Result<()> {
    api.require_key()?;
    api.delete(&format!(
        "/api/v1/research-projects/{project_id}/assets/{asset_id}"
    ))?;
    print_value(fmt, &json!({"status": "deleted", "id": asset_id}), |_| {
        format!("asset {asset_id} deleted")
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn project_import_pdf(
    api: &Api,
    fmt: &Format,
    project_id: &str,
    title: Option<String>,
    uri: Option<String>,
    daas_document_id: Option<String>,
    fuel_role: Option<&str>,
    notes: Option<String>,
    tags: Vec<String>,
) -> anyhow::Result<()> {
    api.require_key()?;
    let mut body = json!({});
    if let Some(t) = title {
        body["title"] = json!(t);
    }
    if let Some(u) = uri {
        body["uri"] = json!(u);
    }
    if let Some(d) = daas_document_id {
        body["daas_document_id"] = json!(d);
    }
    if let Some(r) = fuel_role {
        body["fuel_role"] = json!(r);
    }
    if let Some(n) = notes {
        body["notes"] = json!(n);
    }
    if !tags.is_empty() {
        body["tags"] = json!(tags);
    }
    let paper = api.post(
        &format!("/api/v1/research-projects/{project_id}/papers/from-pdf"),
        &body,
    )?;
    print_value(fmt, &paper, |paper| {
        format!(
            "pdf {} registered on project {} ({})",
            paper["daas_document_id"].as_str().unwrap_or("?"),
            short_id(&paper["project_id"]),
            paper["cached_title"].as_str().unwrap_or("(untitled)"),
        )
    });
    Ok(())
}

pub fn project_search(
    api: &Api,
    fmt: &Format,
    project_id: &str,
    query: &str,
    limit: Option<u32>,
    persist: bool,
) -> anyhow::Result<()> {
    api.require_key()?;
    let mut body = json!({"query": query, "persist": persist});
    if let Some(l) = limit {
        body["limit"] = json!(l);
    }
    let r = api.post(
        &format!("/api/v1/research-projects/{project_id}/search"),
        &body,
    )?;
    print_value(fmt, &r, |r| {
        let hits = r["hits"].as_array().map(|h| h.len()).unwrap_or(0);
        let persisted = r["snapshot_asset_id"]
            .as_str()
            .map(|id| format!(" → snapshot {id}"))
            .unwrap_or_default();
        format!(
            "{} hits in workspace {} (provider: {}){persisted}",
            hits,
            r["workspace_id"].as_str().unwrap_or("?"),
            r["provider"].as_str().unwrap_or("?"),
        )
    });
    Ok(())
}

pub fn runs_list(
    api: &Api,
    fmt: &Format,
    paper: Option<&str>,
    limit: u32,
    all: bool,
) -> anyhow::Result<()> {
    let path = match paper {
        Some(p) => format!("/api/v1/reproduce/runs?paper_id={p}"),
        None => "/api/v1/reproduce/runs".to_string(),
    };
    let items = api.get_paged(&path, limit, all)?;
    print_list(
        fmt,
        &items,
        &["ID", "PAPER", "STATUS", "RUNNER", "CREATED"],
        |r| {
            vec![
                short_id(&r["id"]),
                short_id(&r["paper_id"]),
                r["status"].as_str().unwrap_or("?").to_string(),
                r["runner_principal"].as_str().unwrap_or("?").to_string(),
                r["created_at"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(10)
                    .collect(),
            ]
        },
    );
    Ok(())
}

pub fn runs_get(api: &Api, fmt: &Format, id: &str) -> anyhow::Result<()> {
    let r = api.get(&format!("/api/v1/reproduce/runs/{id}"))?;
    print_value(fmt, &r, |r| {
        format!(
            "run {} — {} (paper {})",
            short_id(&r["id"]),
            r["status"].as_str().unwrap_or("?"),
            short_id(&r["paper_id"]),
        )
    });
    Ok(())
}

pub fn keys_list(api: &Api, fmt: &Format, limit: u32, all: bool) -> anyhow::Result<()> {
    api.require_key()?;
    let items = api.get_paged("/api/v1/keys", limit, all)?;
    print_list(
        fmt,
        &items,
        &["ID", "NAME", "SCOPES", "REVOKED", "CREATED"],
        |k| {
            vec![
                k["id"].as_str().unwrap_or("?").to_string(),
                k["name"].as_str().unwrap_or("").to_string(),
                k["scopes"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default(),
                k["revoked_at"].as_str().unwrap_or("-").to_string(),
                k["created_at"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(10)
                    .collect(),
            ]
        },
    );
    Ok(())
}

pub fn keys_revoke(api: &Api, fmt: &Format, id: &str) -> anyhow::Result<()> {
    api.require_key()?;
    api.delete(&format!("/api/v1/keys/{id}"))?;
    print_value(fmt, &json!({"status": "revoked", "id": id}), |_| {
        format!("key {id} revoked")
    });
    Ok(())
}

/// Compare against `/cli/install`, download, verify sha256, replace atomically.
pub fn self_update(api: &Api, fmt: &Format) -> anyhow::Result<()> {
    let info = api.get("/api/v1/cli/install")?;
    let latest = info["version"].as_str().unwrap_or("");
    let current = env!("CARGO_PKG_VERSION");
    if latest == current {
        print_value(
            fmt,
            &json!({"status": "up_to_date", "version": current}),
            |_| format!("already up to date (v{current})"),
        );
        return Ok(());
    }

    let platform = current_platform();
    let entry = info["platforms"]
        .as_array()
        .and_then(|a| a.iter().find(|p| p["platform"] == json!(platform)))
        .ok_or_else(|| {
            anyhow::anyhow!("no published artifact for platform {platform} (version {latest})")
        })?;
    let url = entry["url"].as_str().unwrap_or_default();
    let sha_url = entry["sha256_url"].as_str().unwrap_or_default();

    eprintln!("downloading {url} …");
    let bin = api.http.get(url).send()?.error_for_status()?.bytes()?;
    let expected = api
        .http
        .get(sha_url)
        .send()?
        .error_for_status()?
        .text()?
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let actual = w_crypto::sha256_hex(&bin);
    if expected.is_empty() || actual != expected {
        anyhow::bail!("sha256 mismatch: expected {expected}, got {actual} — aborting update");
    }

    let exe = std::env::current_exe()?;
    let staging = exe.with_extension("update");
    std::fs::write(&staging, &bin)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&staging, &exe)?;
    print_value(
        fmt,
        &json!({"status": "updated", "from": current, "to": latest, "sha256": actual}),
        |_| format!("updated v{current} → v{latest} ({})", exe.display()),
    );
    Ok(())
}

fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}
