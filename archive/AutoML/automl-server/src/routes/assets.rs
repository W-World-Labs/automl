// ============================================================
// Assets routes — /api/v1/assets/*
// ============================================================

use axum::body::Body;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::{
    extract::{Path, Query, State},
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
        .route("/assets", post(upload_asset).get(list_assets))
        .route("/assets/presign", post(presign_upload))
        .route("/assets/{id}", get(get_asset))
        .route("/assets/{id}/url", get(get_download_url))
        .route("/assets/verify", post(verify_assets))
}

// ── POST /assets — Upload ────────────────────────────────────

async fn upload_asset(
    State(state): State<AppState>,
    user: AuthUser,
    mut multipart: axum::extract::Multipart,
) -> ApiResult<Json<Value>> {
    let mut file_data: Option<(String, Vec<u8>, String)> = None; // (filename, bytes, content_type)
    let mut asset_type = "reference".to_string();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(format!("Multipart error: {}", e)))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            let filename = field.file_name().unwrap_or("unnamed").to_string();
            let ct = field
                .content_type()
                .unwrap_or("application/octet-stream")
                .to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| ApiError::bad_request(format!("Read error: {}", e)))?;
            file_data = Some((filename, bytes.to_vec(), ct));
        } else if name == "type" {
            let val = field.text().await.unwrap_or_default();
            if !["reference", "dataset", "figure", "other"].contains(&val.as_str()) {
                return Err(ApiError::bad_request(format!(
                    "Invalid type '{}'. Must be: reference, dataset, figure, other",
                    val
                )));
            }
            asset_type = val;
        }
    }

    let (filename, bytes, content_type) =
        file_data.ok_or_else(|| ApiError::bad_request("Missing 'file' in form data"))?;

    // Size limit: 100MB
    if bytes.len() > 100 * 1024 * 1024 {
        return Err(ApiError::bad_request("File too large (max 100MB)"));
    }

    // SHA256 hash — bare lowercase-hex, wire-visible (JSON `sha256`, `X-SHA256`
    // header, dedup key); byte-identical to the prior `format!("{:x}", ..)`.
    let sha256 = w_crypto::hash::sha256_hex(&bytes);

    // Dedup check
    let existing: Option<(String, String, i64)> =
        sqlx::query_as("SELECT id, filename, size_bytes FROM assets WHERE sha256 = $1 LIMIT 1")
            .bind(&sha256)
            .fetch_optional(&state.db)
            .await?;

    if let Some((existing_id, existing_filename, existing_size)) = existing {
        return Ok(Json(json!({
            "asset_id": existing_id,
            "filename": existing_filename,
            "sha256": sha256,
            "size": existing_size,
            "deduplicated": true,
            "message": "File already uploaded (SHA256 match)",
        })));
    }

    // Upload to R2
    let asset_id = format!(
        "asset_{}",
        &uuid::Uuid::new_v4().to_string()[..12].replace('-', "")
    );
    let r2_key = format!("assets/{}/{}/{}", asset_type, asset_id, filename);
    let size = bytes.len() as i64;

    state
        .storage
        .put(&r2_key, bytes, &content_type)
        .await
        .map_err(|e| ApiError::internal(format!("Storage error: {}", e)))?;

    // Save metadata
    sqlx::query(
        "INSERT INTO assets (id, scholar_id, owner_user_id, workspace_id, filename, content_type, size_bytes, sha256, r2_key, asset_type, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, CURRENT_TIMESTAMP::text)"
    )
    .bind(&asset_id)
    .bind(&user.scholar_id)
    .bind(&user.owner_user_id)
    .bind(&user.workspace_id)
    .bind(&filename)
    .bind(&content_type)
    .bind(size)
    .bind(&sha256)
    .bind(&r2_key)
    .bind(&asset_type)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "asset_id": asset_id,
        "filename": filename,
        "sha256": sha256,
        "size": size,
        "type": asset_type,
        "url": format!("/api/v1/assets/{}", asset_id),
    })))
}

// ── GET /assets — List ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AssetListParams {
    r#type: Option<String>,
    limit: Option<u32>,
}

async fn list_assets(
    State(state): State<AppState>,
    user: AuthUser,
    Query(params): Query<AssetListParams>,
) -> ApiResult<Json<Value>> {
    let limit = params.limit.unwrap_or(50).min(100) as i64;

    let assets = if let Some(ref t) = params.r#type {
        sqlx::query_as::<_, (String, String, String, i64, String, String)>(
            "SELECT id, filename, content_type, size_bytes, asset_type, created_at FROM assets WHERE scholar_id = $1 AND asset_type = $2 ORDER BY created_at DESC LIMIT $3"
        )
        .bind(&user.scholar_id).bind(t).bind(limit)
        .fetch_all(&state.db).await?
    } else {
        sqlx::query_as::<_, (String, String, String, i64, String, String)>(
            "SELECT id, filename, content_type, size_bytes, asset_type, created_at FROM assets WHERE scholar_id = $1 ORDER BY created_at DESC LIMIT $2"
        )
        .bind(&user.scholar_id).bind(limit)
        .fetch_all(&state.db).await?
    };

    let items: Vec<Value> = assets
        .iter()
        .map(|a| {
            json!({
                "id": a.0, "filename": a.1, "content_type": a.2,
                "size": a.3, "type": a.4, "created_at": a.5,
            })
        })
        .collect();

    Ok(Json(json!({ "assets": items, "total": items.len() })))
}

// ── GET /assets/:id — Download ───────────────────────────────

async fn get_asset(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let meta_only = params.get("meta").map(|v| v == "true").unwrap_or(false);

    let asset: Option<(String, String, String, i64, String, String, String, String)> = sqlx::query_as(
        "SELECT id, filename, content_type, size_bytes, sha256, r2_key, asset_type, created_at FROM assets WHERE id = $1"
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    let (asset_id, filename, content_type, size, sha256, r2_key, asset_type, created_at) =
        asset.ok_or_else(|| ApiError::not_found("Asset not found"))?;

    if meta_only {
        return Ok(Json(json!({
            "asset_id": asset_id,
            "filename": filename,
            "content_type": content_type,
            "size": size,
            "sha256": sha256,
            "type": asset_type,
            "created_at": created_at,
        }))
        .into_response());
    }

    // Download from R2
    let (body, ct) = state
        .storage
        .get(&r2_key)
        .await
        .map_err(|e| ApiError::internal(format!("Storage error: {}", e)))?
        .ok_or_else(|| ApiError::not_found("File not found in storage"))?;

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, ct)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .header(header::CONTENT_LENGTH, size.to_string())
        .header("X-Asset-Id", asset_id)
        .header("X-SHA256", sha256)
        .body(Body::from(body))
        .unwrap()
        .into_response())
}

// ── POST /assets/verify — Batch verify ───────────────────────

#[derive(Debug, Deserialize)]
struct VerifyRequest {
    asset_ids: Vec<String>,
}

async fn verify_assets(
    State(state): State<AppState>,
    Json(body): Json<VerifyRequest>,
) -> ApiResult<Json<Value>> {
    if body.asset_ids.is_empty() {
        return Err(ApiError::bad_request("asset_ids array is required"));
    }
    if body.asset_ids.len() > 100 {
        return Err(ApiError::bad_request("Maximum 100 asset IDs per request"));
    }

    let placeholders: Vec<String> = body
        .asset_ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("${}", i + 1))
        .collect();
    let sql = format!(
        "SELECT id, filename, sha256, asset_type FROM assets WHERE id IN ({})",
        placeholders.join(",")
    );

    let mut query = sqlx::query_as::<_, (String, String, String, String)>(&sql);
    for id in &body.asset_ids {
        query = query.bind(id);
    }
    let found = query.fetch_all(&state.db).await?;

    let found_ids: std::collections::HashSet<&str> = found.iter().map(|r| r.0.as_str()).collect();
    let verified: Vec<Value> = found
        .iter()
        .map(|r| {
            json!({
                "id": r.0, "filename": r.1, "sha256": r.2, "type": r.3,
            })
        })
        .collect();
    let missing: Vec<&str> = body
        .asset_ids
        .iter()
        .filter(|id| !found_ids.contains(id.as_str()))
        .map(|s| s.as_str())
        .collect();

    Ok(Json(json!({
        "verified": verified,
        "missing": missing,
        "total": body.asset_ids.len(),
        "verified_count": verified.len(),
        "missing_count": missing.len(),
        "all_valid": missing.is_empty(),
    })))
}

// ══════════════════════════════════════════════════════════════
// Presigned URL endpoints
// ══════════════════════════════════════════════════════════════

// ── POST /assets/presign — Get presigned upload URL ──────────

#[derive(Debug, Deserialize)]
struct PresignUploadRequest {
    filename: String,
    content_type: Option<String>,
    asset_type: Option<String>,
    /// Expiry in seconds (default: 3600 = 1 hour)
    expires: Option<u64>,
}

async fn presign_upload(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<PresignUploadRequest>,
) -> ApiResult<Json<Value>> {
    let asset_type = body.asset_type.as_deref().unwrap_or("reference");
    if !["reference", "dataset", "figure", "other"].contains(&asset_type) {
        return Err(ApiError::bad_request("Invalid asset_type"));
    }

    let content_type = body.content_type.as_deref().unwrap_or_else(|| {
        mime_guess::from_path(&body.filename)
            .first_raw()
            .unwrap_or("application/octet-stream")
    });
    let expires = body.expires.unwrap_or(3600).min(86400); // max 24h

    let asset_id = format!(
        "asset_{}",
        &uuid::Uuid::new_v4().to_string()[..12].replace('-', "")
    );
    let r2_key = format!("assets/{}/{}/{}", asset_type, asset_id, body.filename);

    let presigned_url = state
        .storage
        .presign_upload(&r2_key, content_type, expires)
        .await
        .map_err(|e| ApiError::internal(format!("Presign error: {}", e)))?;

    // Pre-register asset in DB (status: pending)
    sqlx::query(
        "INSERT INTO assets (id, scholar_id, owner_user_id, workspace_id, filename, content_type, size_bytes, sha256, r2_key, asset_type, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, 0, 'pending', $7, $8, CURRENT_TIMESTAMP::text)"
    )
    .bind(&asset_id)
    .bind(&user.scholar_id)
    .bind(&user.owner_user_id)
    .bind(&user.workspace_id)
    .bind(&body.filename)
    .bind(content_type)
    .bind(&r2_key)
    .bind(asset_type)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "asset_id": asset_id,
        "upload_url": presigned_url,
        "method": "PUT",
        "r2_key": r2_key,
        "content_type": content_type,
        "expires_in": expires,
        "instructions": {
            "curl": format!("curl -X PUT -H 'Content-Type: {}' --data-binary @{} '{}'",
                content_type, body.filename, presigned_url),
        },
    })))
}

// ── GET /assets/:id/url — Get presigned download URL ─────────

async fn get_download_url(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let expires: u64 = params
        .get("expires")
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600)
        .min(86400);

    let asset: Option<(String, String, String, i64, String, String)> = sqlx::query_as(
        "SELECT id, filename, content_type, size_bytes, r2_key, asset_type FROM assets WHERE id = $1"
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    let (asset_id, filename, content_type, size, r2_key, asset_type) =
        asset.ok_or_else(|| ApiError::not_found("Asset not found"))?;

    // If R2_PUBLIC_URL is configured, use base_url pattern (public)
    // Otherwise, generate presigned URL (private)
    let public_url = &state.config.r2_public_url;
    let (download_url, url_type) = if !public_url.is_empty() {
        let base = public_url.trim_end_matches('/');
        (format!("{}/{}", base, r2_key), "public")
    } else {
        let url = state
            .storage
            .presign_download(&r2_key, expires)
            .await
            .map_err(|e| ApiError::internal(format!("Presign error: {}", e)))?;
        (url, "presigned")
    };

    Ok(Json(json!({
        "asset_id": asset_id,
        "filename": filename,
        "content_type": content_type,
        "size": size,
        "type": asset_type,
        "download_url": download_url,
        "url_type": url_type,
        "expires_in": if url_type == "presigned" { Some(expires) } else { None },
    })))
}
