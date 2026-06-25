//! Permission query routes.

use axum::extract::State;
use sqlx::PgPool;

use crate::errors::AppError;
use crate::middleware::auth::AuthUser;
use crate::models::role::PermissionRow;
use crate::response::ApiResponse;

/// `GET /api/v1/permissions`
///
/// List all permissions.
pub async fn list_permissions(
    State(db): State<PgPool>,
    _auth: AuthUser,
) -> Result<ApiResponse<Vec<PermissionRow>>, AppError> {
    let permissions =
        sqlx::query_as::<_, PermissionRow>(
            "SELECT * FROM permissions ORDER BY resource, action",
        )
        .fetch_all(&db)
        .await?;

    Ok(ApiResponse::success("success", Some(permissions)))
}
