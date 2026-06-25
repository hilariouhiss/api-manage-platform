//! Role query routes.

use axum::extract::{Path, State};
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;
use crate::middleware::auth::AuthUser;
use crate::models::role::{PermissionRow, RoleRow, RoleWithPermissions};
use crate::response::ApiResponse;

/// `GET /api/v1/roles`
///
/// List all roles.
pub async fn list_roles(
    State(db): State<PgPool>,
    _auth: AuthUser,
) -> Result<ApiResponse<Vec<RoleRow>>, AppError> {
    let roles = sqlx::query_as::<_, RoleRow>("SELECT * FROM roles ORDER BY created_at ASC")
        .fetch_all(&db)
        .await?;

    Ok(ApiResponse::success("success", Some(roles)))
}

/// `GET /api/v1/roles/:id`
///
/// Get a role with its assigned permissions.
pub async fn get_role(
    State(db): State<PgPool>,
    _auth: AuthUser,
    Path(role_id): Path<Uuid>,
) -> Result<ApiResponse<RoleWithPermissions>, AppError> {
    let role = sqlx::query_as::<_, RoleRow>("SELECT * FROM roles WHERE id = $1")
        .bind(role_id)
        .fetch_optional(&db)
        .await?
        .ok_or_else(|| AppError::not_found("角色"))?;

    let permissions = sqlx::query_as::<_, PermissionRow>(
        "SELECT p.* FROM permissions p
         JOIN role_permissions rp ON rp.permission_id = p.id
         WHERE rp.role_id = $1
         ORDER BY p.resource, p.action",
    )
    .bind(role_id)
    .fetch_all(&db)
    .await?;

    Ok(ApiResponse::success(
        "success",
        Some(RoleWithPermissions {
            role,
            permissions,
        }),
    ))
}
