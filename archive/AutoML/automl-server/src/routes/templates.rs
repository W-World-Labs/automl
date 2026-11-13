// ============================================================
// Templates routes — /api/v1/templates/*
// ============================================================

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/templates", get(list_templates))
        .route("/templates/{id}", get(get_template))
        .route("/templates/fork", post(fork_template))
}

async fn list_templates() -> Json<Value> {
    Json(json!({
        "templates": [
            {
                "id": "genesis",
                "name": "Genesis Paper",
                "description": "Standard research paper template with reproduce.yaml, paper.tex, and refs/",
                "files": [
                    "reproduce.yaml",
                    "paper.tex",
                    "reproduce.sh",
                    "data/iris.csv",
                    "refs/refs-manifest.yaml",
                    "README.md",
                ],
                "download_url": "/paper-template.tar.gz",
                "command": "automl init --template genesis --title 'My Paper' --journal kindergarten",
            }
        ],
        "workflow": {
            "steps": [
                "1. automl login (GitHub + ORCID OAuth)",
                "2. automl init --template genesis --title 'My Paper' --journal kindergarten",
                "3. Edit paper.tex with your research",
                "4. automl asset upload refs/ --type reference",
                "5. Fill asset_ids in refs/refs-manifest.yaml",
                "6. bash reproduce.sh (self-test)",
                "7. automl submit . --journal kindergarten",
            ],
        },
    }))
}

async fn get_template(Path(id): Path<String>) -> ApiResult<Json<Value>> {
    if id != "genesis" {
        return Err(ApiError::not_found(format!("Template '{}' not found", id)));
    }

    Ok(Json(json!({
        "id": "genesis",
        "name": "Genesis Paper",
        "download_url": "/paper-template.tar.gz",
        "description": "Standard paper template with all required files",
    })))
}

#[derive(Debug, Deserialize)]
struct ForkRequest {
    source_paper_id: String,
    title: Option<String>,
}

async fn fork_template(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(body): Json<ForkRequest>,
) -> ApiResult<Json<Value>> {
    // Fetch source paper
    let source: Option<(String, String, String, String)> =
        sqlx::query_as("SELECT id, title, journal_slug, authors FROM papers WHERE id = $1")
            .bind(&body.source_paper_id)
            .fetch_optional(&state.db)
            .await?;

    let (source_id, source_title, journal, _) = source.ok_or_else(|| {
        ApiError::not_found(format!("Source paper '{}' not found", body.source_paper_id))
    })?;

    let new_title = body
        .title
        .unwrap_or_else(|| format!("Fork of: {}", source_title));

    Ok(Json(json!({
        "success": true,
        "source_paper_id": source_id,
        "forked_title": new_title,
        "journal_slug": journal,
        "scaffold": {
            "reproduce_yaml": format!("# Forked from {}\ntitle: {}\njournal: {}\n", source_id, new_title, journal),
            "files_to_create": ["paper.tex", "reproduce.sh", "refs/refs-manifest.yaml"],
        },
        "command": format!("automl init --template genesis --title '{}' --journal {}", new_title, journal),
    })))
}
