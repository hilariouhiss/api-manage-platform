//! Permission query routes.

use axum::extract::State;
use sqlx::PgPool;

use crate::errors::AppError;
use crate::middleware::auth::AuthUser;
use crate::models::role::PermissionRow;
use crate::response::ApiResponse;

/// `GET /api/v1/permissions`
///
/// List all permissions. Requires `permission:list` permission.
pub async fn list_permissions(
    State(db): State<PgPool>,
    AuthUser(claims): AuthUser,
) -> Result<ApiResponse<Vec<PermissionRow>>, AppError> {
    claims.require_permission("permission:list")?;
    let permissions =
        sqlx::query_as::<_, PermissionRow>(
            "SELECT * FROM permissions ORDER BY resource, action",
        )
        .fetch_all(&db)
        .await?;

    Ok(ApiResponse::success("success", Some(permissions)))
}
