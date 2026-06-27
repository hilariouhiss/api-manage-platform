//! Health check route module.
//!
//! Provides a Docker-compatible health check endpoint that verifies
//! connectivity to all critical dependencies (database, Valkey).

use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;

use fred::interfaces::ClientLike;

use crate::response::ApiResponse;
use crate::state::AppState;

/// Dependency health status.
#[derive(Debug, Serialize)]
pub struct HealthStatus {
    /// Overall status: "healthy" or "unhealthy"
    pub status: String,
    /// Database connectivity: "connected" or "disconnected"
    pub database: String,
    /// Valkey connectivity: "connected" or "disconnected"
    pub valkey: String,
}

/// `GET /api/v1/health`
///
/// Checks database (SELECT 1) and Valkey (PING) connectivity.
/// Returns 200 + `"healthy"` when all dependencies are reachable,
/// or 503 + `"unhealthy"` when any dependency is unreachable.
pub async fn health_check(State(state): State<AppState>) -> ApiResponse<HealthStatus> {
    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();
    let valkey_ok = state.valkey.ping::<String>(None).await.is_ok();

    let healthy = db_ok && valkey_ok;

    let health = HealthStatus {
        status: if healthy { "healthy" } else { "unhealthy" }.into(),
        database: if db_ok { "connected" } else { "disconnected" }.into(),
        valkey: if valkey_ok {
            "connected"
        } else {
            "disconnected"
        }
        .into(),
    };

    if healthy {
        ApiResponse::success("health check passed", Some(health))
    } else {
        ApiResponse {
            code: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
            message: "health check failed".into(),
            data: Some(health),
        }
    }
}
