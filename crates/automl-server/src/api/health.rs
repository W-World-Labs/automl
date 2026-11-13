use axum::{extract::State, Json};
use serde_json::{json, Value};
use w_web_kit::health::{health_json_status, HealthInfo, HealthStatus};

use crate::state::AppState;

const INFO: HealthInfo = HealthInfo::new("automl-server", env!("CARGO_PKG_VERSION"), "unknown");

/// Fleet-standard health envelope. Always 200; `status` in the body carries
/// the truth (`ok` / `degraded`) so load balancers keep routing while humans
/// and Radar see the degradation.
#[utoipa::path(
    get,
    path = "/health",
    tag = "system",
    responses((status = 200, description = "Health envelope with per-dependency checks"))
)]
pub async fn health(State(state): State<AppState>) -> Json<Value> {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();
    let status = if db_ok {
        HealthStatus::Ok
    } else {
        HealthStatus::Degraded
    };
    let extras = json!({
        "uptime_seconds": state.started_at.elapsed().as_secs(),
        "checks": { "database": if db_ok { "ok" } else { "error" } },
    });
    Json(health_json_status(&INFO, status, extras))
}
