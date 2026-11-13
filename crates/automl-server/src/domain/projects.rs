//! Research Hub P0: research_projects CRUD + members + audit trail
//! (docs/RESEARCH-HUB-DESIGN.md §12.1/§12.7).
//!
//! The project is the lab work unit. Authorization is two-layered: the global
//! scope check happens in the handler (`require_scope`), the project role in
//! `crate::auth::project_acl` — effective permission = scope ∩ role.

use automl_core::{Page, PageParams};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

/// Audit actions written to `audit_events` (append-only, DB-enforced).
pub mod audit_action {
    pub const PROJECT_CREATED: &str = "project.created";
    pub const PROJECT_UPDATED: &str = "project.updated";
    pub const PROJECT_STATUS_CHANGED: &str = "project.status_changed";
    pub const MEMBER_ADDED: &str = "member.added";
    pub const MEMBER_REMOVED: &str = "member.removed";
    pub const BINDING_BOUND: &str = "binding.bound";
    pub const BINDING_UNBOUND: &str = "binding.unbound";
    pub const ASSET_ADDED: &str = "asset.added";
    pub const ASSET_REMOVED: &str = "asset.removed";
}

/// Who performed a mutation, for the audit trail. API keys act through their
/// accountable owner scholar.
#[derive(Debug, Clone, Copy)]
pub struct ActorRef {
    pub scholar_id: Uuid,
    pub kind: &'static str,
}

impl ActorRef {
    pub fn scholar(id: Uuid) -> Self {
        Self {
            scholar_id: id,
            kind: "scholar",
        }
    }

    pub fn api_key(owner_id: Uuid) -> Self {
        Self {
            scholar_id: owner_id,
            kind: "api_key",
        }
    }
}

/// Project roles ordered Viewer < Member < Owner (§12.1).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRole {
    Viewer,
    Member,
    Owner,
}

impl ProjectRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Member => "member",
            Self::Owner => "owner",
        }
    }

    pub fn parse(s: &str) -> ApiResult<Self> {
        match s {
            "viewer" => Ok(Self::Viewer),
            "member" => Ok(Self::Member),
            "owner" => Ok(Self::Owner),
            _ => Err(ApiError::invalid(format!("unknown project role: {s}"))),
        }
    }
}

/// Lifecycle: `draft → active → archived`; each live state may soft-delete
/// to `deleted` (§12.1). Only deleted is terminal; archived cannot unarchive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Draft,
    Active,
    Archived,
    Deleted,
}

impl ProjectStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Deleted => "deleted",
        }
    }

    pub fn parse(s: &str) -> ApiResult<Self> {
        match s {
            "draft" => Ok(Self::Draft),
            "active" => Ok(Self::Active),
            "archived" => Ok(Self::Archived),
            "deleted" => Ok(Self::Deleted),
            _ => Err(ApiError::invalid(format!("unknown project status: {s}"))),
        }
    }

    /// Legal transitions only (docs/STATE-MACHINES.md — the authority).
    pub fn can_transition(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Draft, Self::Active)
                | (Self::Active, Self::Archived)
                | (Self::Draft, Self::Deleted)
                | (Self::Active, Self::Deleted)
                | (Self::Archived, Self::Deleted)
        )
    }
}

/// `private` = members only. `lab` is reserved for the (future) lab/org concept
/// and behaves as members-only until labs exist. `public_meta` exposes project
/// an explicit metadata allowlist to authenticated principals with projects:read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectVisibility {
    Private,
    Lab,
    PublicMeta,
}

impl ProjectVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Lab => "lab",
            Self::PublicMeta => "public_meta",
        }
    }

    pub fn parse(s: &str) -> ApiResult<Self> {
        match s {
            "private" => Ok(Self::Private),
            "lab" => Ok(Self::Lab),
            "public_meta" => Ok(Self::PublicMeta),
            _ => Err(ApiError::invalid(format!("unknown visibility: {s}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ResearchProject {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub visibility: ProjectVisibility,
    pub status: ProjectStatus,
    /// Reserved lab/org pointer (P0 stub — no lab table yet).
    pub lab_id: Option<Uuid>,
    /// Unverified external workspace pointer; no access grant or downstream call.
    pub w_daas_workspace_id: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateProjectRequest {
    /// Creator-unique slug (`^[a-z0-9][a-z0-9-]{0,63}$`); derived from `title`
    /// when omitted.
    #[serde(default)]
    pub slug: Option<String>,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    /// Defaults to `private`.
    #[serde(default)]
    pub visibility: Option<ProjectVisibility>,
    #[serde(default)]
    pub lab_id: Option<Uuid>,
    #[serde(default)]
    pub w_daas_workspace_id: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PatchProjectRequest {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub slug: Option<String>,
    pub visibility: Option<ProjectVisibility>,
    /// Lifecycle action: `active` (activate), `archived` (archive),
    /// `deleted` (soft delete). Illegal transitions → 409.
    pub status: Option<ProjectStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ProjectMember {
    pub id: Uuid,
    pub project_id: Uuid,
    /// `scholar` (agent ACL enrollment is deferred in P0).
    pub principal_type: String,
    pub scholar_id: Option<Uuid>,
    /// Joined scholars.w_id for scholar principals.
    pub scholar_w_id: Option<String>,
    pub agent_id: Option<Uuid>,
    /// Reserved for legacy agent rows; agent authorization is not supported in P0.
    pub agent_name: Option<String>,
    pub role: ProjectRole,
    pub added_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AddMemberRequest {
    /// Scholar to add: W-ID handle or scholar UUID.
    #[serde(default)]
    pub scholar: Option<String>,
    /// Reserved compatibility field. Agent ACL enrollment is not supported in P0.
    #[serde(default)]
    pub agent: Option<Uuid>,
    pub role: ProjectRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AuditEvent {
    pub id: Uuid,
    pub project_id: Uuid,
    pub actor_id: Option<Uuid>,
    /// `scholar` | `api_key` | `system`.
    pub actor_kind: String,
    pub action: String,
    pub detail: Value,
    pub created_at: DateTime<Utc>,
}

// ── Projects ────────────────────────────────────────────────────────────────

pub async fn create(
    pool: &PgPool,
    actor: ActorRef,
    req: CreateProjectRequest,
) -> ApiResult<ResearchProject> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err(ApiError::invalid("title must not be empty"));
    }
    let slug = match req.slug.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => slugify(title),
    };
    if !valid_slug(&slug) {
        return Err(ApiError::invalid(
            "slug must match ^[a-z0-9][a-z0-9-]{0,63}$ (or be derivable from title)",
        ));
    }
    let visibility = req.visibility.unwrap_or(ProjectVisibility::Private);
    let w_daas_workspace_id = req
        .w_daas_workspace_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut tx = pool.begin().await?;
    let row = match sqlx::query_as::<_, ProjectRow>(
        r#"
        INSERT INTO research_projects (
            slug, title, summary, visibility, lab_id, w_daas_workspace_id, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, slug, title, summary, visibility, status, lab_id,
                  w_daas_workspace_id, created_by, created_at, updated_at
        "#,
    )
    .bind(&slug)
    .bind(title)
    .bind(req.summary.trim())
    .bind(visibility.as_str())
    .bind(req.lab_id)
    .bind(w_daas_workspace_id)
    .bind(actor.scholar_id)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(row) => row,
        Err(e) if is_unique_violation(&e) => {
            return Err(ApiError::conflict(format!(
                "slug '{slug}' is already used by one of your projects"
            )));
        }
        Err(e) => return Err(e.into()),
    };
    let project_id = row.id;

    // Creator is the initial Owner member; grant lands atomically with the row.
    sqlx::query(
        r#"
        INSERT INTO project_members
            (project_id, principal_type, scholar_id, role, added_by)
        VALUES ($1, 'scholar', $2, 'owner', $2)
        "#,
    )
    .bind(project_id)
    .bind(actor.scholar_id)
    .execute(&mut *tx)
    .await?;

    // A P0-style workspace pointer supplied at create becomes an unverified
    // P2 binding row so the column and the binding log tell one story.
    if let Some(ws) = w_daas_workspace_id {
        sqlx::query(
            r#"
            INSERT INTO project_bindings (project_id, workspace_id, created_by, verified)
            VALUES ($1, $2, $3, false)
            "#,
        )
        .bind(project_id)
        .bind(ws)
        .bind(actor.scholar_id)
        .execute(&mut *tx)
        .await?;
    }

    record_audit(
        &mut tx,
        project_id,
        actor,
        audit_action::PROJECT_CREATED,
        serde_json::json!({"slug": slug, "title": title}),
    )
    .await?;
    tx.commit().await?;
    Ok(row.into())
}

pub async fn get(pool: &PgPool, id: Uuid) -> ApiResult<ResearchProject> {
    let row = sqlx::query_as::<_, ProjectRow>(
        r#"
        SELECT id, slug, title, summary, visibility, status, lab_id,
               w_daas_workspace_id, created_by, created_at, updated_at
        FROM research_projects WHERE id = $1 AND status <> 'deleted'
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("project not found"))?;
    Ok(row.into())
}

/// Existence + liveness for ACL checks: missing or soft-deleted → 404.
pub async fn ensure_live(pool: &PgPool, id: Uuid) -> ApiResult<()> {
    let alive: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM research_projects WHERE id = $1 AND status <> 'deleted')",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    if alive {
        Ok(())
    } else {
        Err(ApiError::not_found("project not found"))
    }
}

/// Projects the scholar belongs to (any role), newest first, keyset-paginated.
pub async fn list_mine(
    pool: &PgPool,
    scholar_id: Uuid,
    params: &PageParams,
) -> ApiResult<Page<ResearchProject>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (after_ts, after_id) = match params.cursor.as_deref() {
        Some(c) => {
            let (ts, id) = super::cursor::decode(c)?;
            (Some(ts), Some(id))
        }
        None => (None, None),
    };
    let rows = sqlx::query_as::<_, ProjectRow>(
        r#"
        SELECT p.id, p.slug, p.title, p.summary, p.visibility, p.status, p.lab_id,
               p.w_daas_workspace_id, p.created_by, p.created_at, p.updated_at
        FROM research_projects p
        JOIN project_members m ON m.project_id = p.id
        WHERE m.principal_type = 'scholar' AND m.scholar_id = $1
          AND p.status <> 'deleted'
          AND ($2::timestamptz IS NULL OR (p.created_at, p.id) < ($2, $3))
        ORDER BY p.created_at DESC, p.id DESC
        LIMIT $4
        "#,
    )
    .bind(scholar_id)
    .bind(after_ts)
    .bind(after_id)
    .bind(fetch)
    .fetch_all(pool)
    .await?;
    Ok(Page::from_rows(
        rows.into_iter().map(ResearchProject::from).collect(),
        limit,
        |p| super::cursor::encode(p.created_at, p.id),
    ))
}

/// Field edits and lifecycle. `status` moves go through the state machine —
/// illegal transitions are 409 (docs/STATE-MACHINES.md).
pub async fn patch(
    pool: &PgPool,
    id: Uuid,
    actor: ActorRef,
    req: PatchProjectRequest,
) -> ApiResult<ResearchProject> {
    let mut tx = pool.begin().await?;
    let current = lock_project_for_owner(&mut tx, id, actor.scholar_id).await?;
    let status_from = current.status.clone();
    let mut changed: Vec<&str> = Vec::new();
    if let Some(ref title) = req.title {
        if title.trim().is_empty() {
            return Err(ApiError::invalid("title must not be empty"));
        }
        if title.trim() != current.title {
            changed.push("title");
        }
    }
    if req.summary.as_deref().is_some_and(|v| v != current.summary) {
        changed.push("summary");
    }
    if let Some(ref slug) = req.slug {
        if !valid_slug(slug.trim()) {
            return Err(ApiError::invalid(
                "slug must match ^[a-z0-9][a-z0-9-]{0,63}$",
            ));
        }
        if slug.trim() != current.slug {
            changed.push("slug");
        }
    }
    if req
        .visibility
        .is_some_and(|v| v.as_str() != current.visibility)
    {
        changed.push("visibility");
    }

    let status_to = match req.status {
        Some(to) if to.as_str() != current.status => {
            let from = ProjectStatus::parse(&current.status)?;
            if !from.can_transition(to) {
                return Err(ApiError::conflict(format!(
                    "illegal project status transition: {} → {}",
                    from.as_str(),
                    to.as_str()
                )));
            }
            Some(to)
        }
        _ => None,
    };

    let mut query = QueryBuilder::<Postgres>::new("UPDATE research_projects SET ");
    let mut fields = query.separated(", ");
    if changed.contains(&"title") {
        fields
            .push("title = ")
            .push_bind_unseparated(req.title.as_deref().unwrap().trim());
    }
    if changed.contains(&"summary") {
        fields
            .push("summary = ")
            .push_bind_unseparated(req.summary.as_deref().unwrap());
    }
    if changed.contains(&"slug") {
        fields
            .push("slug = ")
            .push_bind_unseparated(req.slug.as_deref().unwrap().trim());
    }
    if changed.contains(&"visibility") {
        fields
            .push("visibility = ")
            .push_bind_unseparated(req.visibility.unwrap().as_str());
    }
    if let Some(to) = status_to {
        fields.push("status = ").push_bind_unseparated(to.as_str());
    }
    let has_update = !changed.is_empty() || status_to.is_some();
    if has_update {
        query.push(", updated_at = now() WHERE id = ").push_bind(id);
    }
    let row = if has_update {
        match query.push(" RETURNING id, slug, title, summary, visibility, status, lab_id, w_daas_workspace_id, created_by, created_at, updated_at")
            .build_query_as::<ProjectRow>().fetch_one(&mut *tx).await {
            Ok(row) => row,
            Err(e) if is_unique_violation(&e) => return Err(ApiError::conflict("project slug is already in use")),
            Err(e) => return Err(e.into()),
        }
    } else {
        current
    };

    if let Some(to) = status_to {
        record_audit(
            &mut tx,
            id,
            actor,
            audit_action::PROJECT_STATUS_CHANGED,
            serde_json::json!({
                "from": status_from,
                "to": to.as_str(),
            }),
        )
        .await?;
    }
    if !changed.is_empty() {
        record_audit(
            &mut tx,
            id,
            actor,
            audit_action::PROJECT_UPDATED,
            serde_json::json!({"changed": changed}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(row.into())
}

// ── Members ─────────────────────────────────────────────────────────────────

/// Caller's project role via their accountable scholar member row. Scholar
/// sessions and scholar-owned API keys use the same membership. Returns
/// `None` for non-members.
pub async fn member_role<'e, E>(
    ex: E,
    project_id: Uuid,
    scholar_id: Uuid,
) -> ApiResult<Option<ProjectRole>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let role: Option<String> = sqlx::query_scalar(
        r#"
        SELECT role FROM project_members
        WHERE project_id = $1 AND principal_type = 'scholar' AND scholar_id = $2
        "#,
    )
    .bind(project_id)
    .bind(scholar_id)
    .fetch_optional(ex)
    .await?;
    role.map(|r| ProjectRole::parse(&r)).transpose()
}

pub async fn list_members(
    pool: &PgPool,
    project_id: Uuid,
    params: &PageParams,
) -> ApiResult<Page<ProjectMember>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (after_ts, after_id) = match params.cursor.as_deref() {
        Some(c) => {
            let (ts, id) = super::cursor::decode(c)?;
            (Some(ts), Some(id))
        }
        None => (None, None),
    };
    let rows = sqlx::query_as::<_, MemberRow>(
        r#"
        SELECT m.id, m.project_id, m.principal_type, m.scholar_id, m.agent_id,
               m.role, m.added_by, m.created_at,
               s.w_id AS scholar_w_id, a.name AS agent_name
        FROM project_members m
        LEFT JOIN scholars s ON s.id = m.scholar_id
        LEFT JOIN agents a ON a.id = m.agent_id
        WHERE m.project_id = $1
          AND ($2::timestamptz IS NULL OR (m.created_at, m.id) < ($2, $3))
        ORDER BY m.created_at DESC, m.id DESC
        LIMIT $4
        "#,
    )
    .bind(project_id)
    .bind(after_ts)
    .bind(after_id)
    .bind(fetch)
    .fetch_all(pool)
    .await?;
    Ok(Page::from_rows(
        rows.into_iter().map(ProjectMember::from).collect(),
        limit,
        |m| super::cursor::encode(m.created_at, m.id),
    ))
}

/// Add a scholar (by W-ID handle or UUID). Agent ACL enrollment is deferred
/// until credentials can be bound to verified agent identity.
pub async fn add_member(
    pool: &PgPool,
    project_id: Uuid,
    actor: ActorRef,
    req: AddMemberRequest,
) -> ApiResult<ProjectMember> {
    let mut tx = pool.begin().await?;
    lock_project_for_owner(&mut tx, project_id, actor.scholar_id).await?;
    if req.agent.is_some() {
        return Err(ApiError::not_implemented(
            "agent project ACL enrollment is deferred; P0 supports scholar-delegated credentials only",
        ));
    }
    let Some(ref ident) = req.scholar else {
        return Err(ApiError::invalid("`scholar` is required"));
    };

    let scholar = resolve_scholar(&mut tx, ident).await?;
    let detail = serde_json::json!({"scholar_id": scholar, "role": req.role.as_str()});

    let member_id = Uuid::new_v4();
    match sqlx::query(
        r#"
        INSERT INTO project_members
            (id, project_id, principal_type, scholar_id, agent_id, role, added_by)
        VALUES ($1, $2, 'scholar', $3, NULL, $4, $5)
        "#,
    )
    .bind(member_id)
    .bind(project_id)
    .bind(scholar)
    .bind(req.role.as_str())
    .bind(actor.scholar_id)
    .execute(&mut *tx)
    .await
    {
        Ok(_) => {}
        Err(e) if is_unique_violation(&e) => {
            return Err(ApiError::conflict("principal is already a project member"));
        }
        Err(e) => return Err(e.into()),
    }

    record_audit(
        &mut tx,
        project_id,
        actor,
        audit_action::MEMBER_ADDED,
        detail,
    )
    .await?;
    tx.commit().await?;
    get_member(pool, project_id, member_id).await
}

/// Remove a member row. The last owner cannot be removed (the project would
/// become ungovernable) → 409.
pub async fn remove_member(
    pool: &PgPool,
    project_id: Uuid,
    member_id: Uuid,
    actor: ActorRef,
) -> ApiResult<()> {
    let mut tx = pool.begin().await?;
    lock_project_for_owner(&mut tx, project_id, actor.scholar_id).await?;
    let member = sqlx::query_as::<_, MemberRow>(
        r#"
        SELECT m.id, m.project_id, m.principal_type, m.scholar_id, m.agent_id,
               m.role, m.added_by, m.created_at,
               s.w_id AS scholar_w_id, a.name AS agent_name
        FROM project_members m
        LEFT JOIN scholars s ON s.id = m.scholar_id
        LEFT JOIN agents a ON a.id = m.agent_id
        WHERE m.id = $1 AND m.project_id = $2
        "#,
    )
    .bind(member_id)
    .bind(project_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::not_found("member not found"))?;

    if member.role == "owner" {
        let owners: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM project_members WHERE project_id = $1 AND principal_type = 'scholar' AND role = 'owner'",
        )
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await?;
        if owners <= 1 {
            return Err(ApiError::conflict("cannot remove the last project owner"));
        }
    }

    sqlx::query("DELETE FROM project_members WHERE id = $1")
        .bind(member_id)
        .execute(&mut *tx)
        .await?;
    record_audit(
        &mut tx,
        project_id,
        actor,
        audit_action::MEMBER_REMOVED,
        serde_json::json!({
            "scholar_id": member.scholar_id,
            "agent_id": member.agent_id,
            "role": member.role,
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn get_member(pool: &PgPool, project_id: Uuid, member_id: Uuid) -> ApiResult<ProjectMember> {
    let row = sqlx::query_as::<_, MemberRow>(
        r#"
        SELECT m.id, m.project_id, m.principal_type, m.scholar_id, m.agent_id,
               m.role, m.added_by, m.created_at,
               s.w_id AS scholar_w_id, a.name AS agent_name
        FROM project_members m
        LEFT JOIN scholars s ON s.id = m.scholar_id
        LEFT JOIN agents a ON a.id = m.agent_id
        WHERE m.id = $1 AND m.project_id = $2
        "#,
    )
    .bind(member_id)
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

async fn resolve_scholar(tx: &mut Transaction<'_, Postgres>, ident: &str) -> ApiResult<Uuid> {
    let ident = ident.trim();
    let row: Option<Uuid> = match Uuid::parse_str(ident) {
        Ok(id) => {
            sqlx::query_scalar("SELECT id FROM scholars WHERE id = $1")
                .bind(id)
                .fetch_optional(&mut **tx)
                .await?
        }
        Err(_) => {
            sqlx::query_scalar("SELECT id FROM scholars WHERE w_id = $1")
                .bind(ident)
                .fetch_optional(&mut **tx)
                .await?
        }
    };
    row.ok_or_else(|| ApiError::invalid(format!("unknown scholar: {ident}")))
}

/// Lock the project mutation aggregate and recheck both liveness and the
/// accountable human's Owner membership inside the same transaction.
pub(crate) async fn lock_project_for_owner(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor_id: Uuid,
) -> ApiResult<ProjectRow> {
    let project = sqlx::query_as::<_, ProjectRow>(
        r#"SELECT id, slug, title, summary, visibility, status, lab_id,
                  w_daas_workspace_id, created_by, created_at, updated_at
           FROM research_projects WHERE id = $1 FOR UPDATE"#,
    )
    .bind(project_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| ApiError::not_found("project not found"))?;
    if project.status == "deleted" {
        return Err(ApiError::not_found("project not found"));
    }
    match member_role(&mut **tx, project_id, actor_id).await? {
        Some(ProjectRole::Owner) => Ok(project),
        _ => Err(ApiError::forbidden("project Owner role required")),
    }
}

// ── Audit ───────────────────────────────────────────────────────────────────

pub(crate) async fn record_audit(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor: ActorRef,
    action: &str,
    detail: Value,
) -> ApiResult<()> {
    sqlx::query(
        r#"
        INSERT INTO audit_events (project_id, actor_id, actor_kind, action, detail)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(project_id)
    .bind(actor.scholar_id)
    .bind(actor.kind)
    .bind(action)
    .bind(&detail)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn list_audit(
    pool: &PgPool,
    project_id: Uuid,
    params: &PageParams,
) -> ApiResult<Page<AuditEvent>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (after_ts, after_id) = match params.cursor.as_deref() {
        Some(c) => {
            let (ts, id) = super::cursor::decode(c)?;
            (Some(ts), Some(id))
        }
        None => (None, None),
    };
    let rows = sqlx::query_as::<_, AuditRow>(
        r#"
        SELECT id, project_id, actor_id, actor_kind, action, detail, created_at
        FROM audit_events
        WHERE project_id = $1
          AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
        ORDER BY created_at DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(project_id)
    .bind(after_ts)
    .bind(after_id)
    .bind(fetch)
    .fetch_all(pool)
    .await?;
    Ok(Page::from_rows(
        rows.into_iter().map(AuditEvent::from).collect(),
        limit,
        |e| super::cursor::encode(e.created_at, e.id),
    ))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

pub(crate) fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(dbe) if dbe.is_unique_violation())
}

fn valid_slug(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// Derive a slug from a title: lowercase, runs of non-alphanumeric → `-`,
/// no leading/trailing dash, ≤64 chars.
fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for c in title.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out.truncate(64);
    while out.ends_with('-') {
        out.pop();
    }
    out
}

// ── Row mappings ────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
pub(crate) struct ProjectRow {
    id: Uuid,
    slug: String,
    title: String,
    summary: String,
    visibility: String,
    status: String,
    lab_id: Option<Uuid>,
    w_daas_workspace_id: Option<String>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ProjectRow> for ResearchProject {
    fn from(r: ProjectRow) -> Self {
        Self {
            id: r.id,
            slug: r.slug,
            title: r.title,
            summary: r.summary,
            visibility: ProjectVisibility::parse(&r.visibility)
                .unwrap_or(ProjectVisibility::Private),
            status: ProjectStatus::parse(&r.status).unwrap_or(ProjectStatus::Draft),
            lab_id: r.lab_id,
            w_daas_workspace_id: r.w_daas_workspace_id,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    id: Uuid,
    project_id: Uuid,
    principal_type: String,
    scholar_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    scholar_w_id: Option<String>,
    agent_name: Option<String>,
    role: String,
    added_by: Option<Uuid>,
    created_at: DateTime<Utc>,
}

impl From<MemberRow> for ProjectMember {
    fn from(r: MemberRow) -> Self {
        Self {
            id: r.id,
            project_id: r.project_id,
            principal_type: r.principal_type,
            scholar_id: r.scholar_id,
            scholar_w_id: r.scholar_w_id,
            agent_id: r.agent_id,
            agent_name: r.agent_name,
            role: ProjectRole::parse(&r.role).unwrap_or(ProjectRole::Viewer),
            added_by: r.added_by,
            created_at: r.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct AuditRow {
    id: Uuid,
    project_id: Uuid,
    actor_id: Option<Uuid>,
    actor_kind: String,
    action: String,
    detail: Value,
    created_at: DateTime<Utc>,
}

impl From<AuditRow> for AuditEvent {
    fn from(r: AuditRow) -> Self {
        Self {
            id: r.id,
            project_id: r.project_id,
            actor_id: r.actor_id,
            actor_kind: r.actor_kind,
            action: r.action,
            detail: r.detail,
            created_at: r.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_status_machine() {
        use ProjectStatus::*;
        // Legal path.
        assert!(Draft.can_transition(Active));
        assert!(Active.can_transition(Archived));
        // Soft delete from every non-terminal state.
        for s in [Draft, Active, Archived] {
            assert!(s.can_transition(Deleted), "{s:?} -> deleted");
        }
        // Illegal transitions mirror docs/STATE-MACHINES.md.
        assert!(!Draft.can_transition(Archived));
        assert!(!Archived.can_transition(Active));
        assert!(!Archived.can_transition(Draft));
        assert!(!Active.can_transition(Draft));
        for t in [Draft, Active, Archived, Deleted] {
            assert!(!Deleted.can_transition(t), "deleted is terminal");
        }
        assert!(ProjectStatus::parse("bogus").is_err());
    }

    #[test]
    fn role_ordering() {
        assert!(ProjectRole::Owner > ProjectRole::Member);
        assert!(ProjectRole::Member > ProjectRole::Viewer);
        assert!(ProjectRole::Viewer >= ProjectRole::Viewer);
    }

    #[test]
    fn slug_rules() {
        assert!(valid_slug("ml-theory"));
        assert!(valid_slug("a"));
        assert!(!valid_slug(""));
        assert!(!valid_slug("-lead"));
        assert!(!valid_slug("Upper"));
        assert!(!valid_slug(&"x".repeat(65)));
        assert_eq!(slugify("My Cool Project!"), "my-cool-project");
        assert_eq!(slugify("  --weird--  "), "weird");
        assert_eq!(slugify("！！！"), "");
        assert!(slugify(&"long ".repeat(30)).len() <= 64);
    }
}
