//! User management routes: CRUD, self-profile, and paginated listing.

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use crate::errors::AppError;
use crate::middleware::auth::AuthUser;
use crate::models::auth::JwtClaims;
use crate::models::user::{
    CreateUserPayload, PaginatedResponse, UpdateUserPayload, UserCursor,
    UserListItem, UserResponse, UserRow,
};
use crate::response::ApiResponse;

/// Default page size for user list.
const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

// ── Query params for pagination ───────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListUsersQuery {
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

// ── Helpers ───────────────────────────────────────────────────

/// Check whether `actor` (the authenticated user) is allowed to
/// operate on `target_user_id`. Admins can only manage users who
/// have the "user" role (not other admins or system_admins).
///
/// Accepts any executor (`&PgPool` or `&mut Transaction`) so the
/// check can run inside a database transaction to avoid TOCTOU races.
async fn check_manage_scope<'a>(
    executor: impl sqlx::Executor<'a, Database = sqlx::Postgres>,
    actor: &JwtClaims,
    target_user_id: Uuid,
) -> Result<(), AppError> {
    // system_admin can manage anyone
    if actor.has_role("system_admin") {
        return Ok(());
    }

    // admin can only manage users who have ONLY the "user" role
    if actor.has_role("admin") {
        let dangerous_roles: Vec<String> = sqlx::query_scalar(
            "SELECT r.name FROM roles r
             JOIN user_roles ur ON ur.role_id = r.id
             WHERE ur.user_id = $1 AND r.name IN ('admin', 'system_admin')",
        )
        .bind(target_user_id)
        .fetch_all(executor)
        .await?;

        if !dangerous_roles.is_empty() {
            return Err(AppError::Forbidden);
        }
        return Ok(());
    }

    Err(AppError::Forbidden)
}

// ── Handlers ──────────────────────────────────────────────────

/// `GET /api/v1/users/me`
///
/// Returns the authenticated user's profile.
pub async fn me(
    State(db): State<PgPool>,
    AuthUser(claims): AuthUser,
) -> Result<ApiResponse<UserResponse>, AppError> {
    let row = sqlx::query_as::<_, UserRow>(
        "SELECT * FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(claims.sub)
    .fetch_optional(&db)
    .await?
    .ok_or_else(|| AppError::not_found("用户"))?;

    Ok(ApiResponse::success("success", Some(row.into())))
}

/// `PUT /api/v1/users/me`
///
/// Updates the authenticated user's own profile.
/// Cannot change username or password via this endpoint.
pub async fn update_me(
    State(db): State<PgPool>,
    AuthUser(claims): AuthUser,
    Json(payload): Json<UpdateUserPayload>,
) -> Result<ApiResponse<UserResponse>, AppError> {
    // Reject role_ids in self-update (admin-only field)
    if payload.role_ids.is_some() {
        return Err(AppError::Forbidden);
    }

    payload
        .validate()
        .map_err(AppError::Validation)?;

    // Check uniqueness constraints
    if let Some(ref email) = payload.email {
        let conflict = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM users WHERE email = $1 AND id != $2 AND deleted_at IS NULL)",
        )
        .bind(email)
        .bind(claims.sub)
        .fetch_one(&db)
        .await?;

        if conflict {
            return Err(AppError::Conflict("邮箱已存在".into()));
        }
    }

    if let Some(ref phone) = payload.phone {
        let conflict = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM users WHERE phone = $1 AND id != $2 AND deleted_at IS NULL)",
        )
        .bind(phone)
        .bind(claims.sub)
        .fetch_one(&db)
        .await?;

        if conflict {
            return Err(AppError::Conflict("手机号已存在".into()));
        }
    }

    let row = sqlx::query_as::<_, UserRow>(
        r#"UPDATE users SET
            display_name = COALESCE($1, display_name),
            email = COALESCE($2, email),
            phone = COALESCE($3, phone),
            avatar_url = COALESCE($4, avatar_url),
            self_intro = COALESCE($5, self_intro),
            updated_at = now(),
            updated_by = $6
          WHERE id = $7 AND deleted_at IS NULL
          RETURNING *"#,
    )
    .bind(&payload.display_name)
    .bind(&payload.email)
    .bind(&payload.phone)
    .bind(&payload.avatar_url)
    .bind(&payload.self_intro)
    .bind(claims.sub)
    .bind(claims.sub)
    .fetch_optional(&db)
    .await?
    .ok_or_else(|| AppError::not_found("用户"))?;

    Ok(ApiResponse::success("更新成功", Some(row.into())))
}

/// `GET /api/v1/users`
///
/// Paginated user list. Requires `user:list` permission.
/// Uses keyset (cursor-based) pagination on (created_at DESC, id DESC).
pub async fn list_users(
    State(db): State<PgPool>,
    AuthUser(claims): AuthUser,
    Query(params): Query<ListUsersQuery>,
) -> Result<ApiResponse<PaginatedResponse<UserListItem>>, AppError> {
    claims.require_permission("user:list")?;

    let limit = params
        .limit
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);

    // Fetch one extra row to determine has_more
    let fetch_limit = limit + 1;

    let rows: Vec<UserRow> = if let Some(ref cursor_str) = params.cursor {
        let cursor = UserCursor::decode(cursor_str)?;
        sqlx::query_as(
            "SELECT * FROM users
             WHERE deleted_at IS NULL
               AND (created_at, id) < ($1, $2)
             ORDER BY created_at DESC, id DESC
             LIMIT $3",
        )
        .bind(cursor.created_at)
        .bind(cursor.id)
        .bind(fetch_limit)
        .fetch_all(&db)
        .await?
    } else {
        sqlx::query_as(
            "SELECT * FROM users
             WHERE deleted_at IS NULL
             ORDER BY created_at DESC, id DESC
             LIMIT $1",
        )
        .bind(fetch_limit)
        .fetch_all(&db)
        .await?
    };

    let has_more = rows.len() as i64 > limit;
    let data: Vec<UserRow> = if has_more {
        rows.into_iter().take(limit as usize).collect()
    } else {
        rows
    };

    let next_cursor = if has_more {
        data.last().map(|row| {
            UserCursor {
                created_at: row.created_at,
                id: row.id,
            }
            .encode()
        })
    } else {
        None
    };

    let items: Vec<UserListItem> = data.into_iter().map(UserListItem::from).collect();

    Ok(ApiResponse::success(
        "success",
        Some(PaginatedResponse {
            data: items,
            next_cursor,
            has_more,
        }),
    ))
}

/// `GET /api/v1/users/:id`
///
/// Get a single user by ID. Requires `user:list` permission.
pub async fn get_user(
    State(db): State<PgPool>,
    AuthUser(claims): AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<ApiResponse<UserResponse>, AppError> {
    claims.require_permission("user:list")?;

    let row = sqlx::query_as::<_, UserRow>(
        "SELECT * FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(&db)
    .await?
    .ok_or_else(|| AppError::not_found("用户"))?;

    Ok(ApiResponse::success("success", Some(row.into())))
}

/// `POST /api/v1/users`
///
/// Create a new user (admin only). Requires `user:create` permission.
pub async fn create_user(
    State(db): State<PgPool>,
    AuthUser(claims): AuthUser,
    Json(payload): Json<CreateUserPayload>,
) -> Result<ApiResponse<UserResponse>, AppError> {
    claims.require_permission("user:create")?;

    payload
        .validate()
        .map_err(AppError::Validation)?;

    let password_hash =
        crate::auth::password::hash_password(&payload.password).map_err(AppError::Internal)?;

    // Begin transaction — checks and inserts are atomic
    let mut tx = db.begin().await?;

    // Check uniqueness
    let conflict = sqlx::query_scalar::<_, String>(
        "SELECT
            CASE
                WHEN EXISTS (SELECT 1 FROM users WHERE username = $1 AND deleted_at IS NULL) THEN '用户名已存在'
                WHEN EXISTS (SELECT 1 FROM users WHERE email = $2 AND deleted_at IS NULL) THEN '邮箱已存在'
                WHEN EXISTS (SELECT 1 FROM users WHERE phone = $3 AND deleted_at IS NULL) THEN '手机号已存在'
            END",
    )
    .bind(&payload.username)
    .bind(&payload.email)
    .bind(&payload.phone)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(msg) = conflict {
        return Err(AppError::Conflict(msg));
    }

    // Insert user
    let row = sqlx::query_as::<_, UserRow>(
        "INSERT INTO users (display_name, username, password_hash, email, phone, avatar_url, self_intro, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *",
    )
    .bind(&payload.display_name)
    .bind(&payload.username)
    .bind(&password_hash)
    .bind(&payload.email)
    .bind(&payload.phone)
    .bind(&payload.avatar_url)
    .bind(&payload.self_intro)
    .bind(claims.sub)
    .fetch_one(&mut *tx)
    .await?;

    // Assign default "user" role
    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id)
         SELECT $1, id FROM roles WHERE name = 'user'",
    )
    .bind(row.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(ApiResponse::success("创建成功", Some(row.into())))
}

/// `PUT /api/v1/users/:id`
///
/// Update a user. Requires `user:manage` permission.
/// Scope: admin can only manage users with "user" role.
/// Admin cannot assign system_admin or admin roles.
pub async fn update_user(
    State(db): State<PgPool>,
    AuthUser(claims): AuthUser,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<UpdateUserPayload>,
) -> Result<ApiResponse<UserResponse>, AppError> {
    claims.require_permission("user:manage")?;

    payload
        .validate()
        .map_err(AppError::Validation)?;

    // Single transaction for scope check + role validation + profile + role updates
    let mut tx = db.begin().await?;

    // Scope check (inside transaction to avoid TOCTOU)
    check_manage_scope(&mut *tx, &claims, user_id).await?;

    // Validate role assignment: admin cannot assign system_admin or admin roles
    if let Some(ref role_ids) = payload.role_ids
        && !role_ids.is_empty()
        && !claims.has_role("system_admin")
    {
            let forbidden_roles: Vec<String> = sqlx::query_scalar(
                "SELECT r.name FROM roles r
                 WHERE r.id = ANY($1) AND r.name IN ('admin', 'system_admin')",
            )
            .bind(role_ids)
            .fetch_all(&mut *tx)
            .await?;

        if !forbidden_roles.is_empty() {
            return Err(AppError::Forbidden);
        }
    }

    // Check uniqueness (inside transaction)
    if let Some(ref email) = payload.email {
        let conflict = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM users WHERE email = $1 AND id != $2 AND deleted_at IS NULL)",
        )
        .bind(email)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;

        if conflict {
            return Err(AppError::Conflict("邮箱已存在".into()));
        }
    }

    if let Some(ref phone) = payload.phone {
        let conflict = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM users WHERE phone = $1 AND id != $2 AND deleted_at IS NULL)",
        )
        .bind(phone)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;

        if conflict {
            return Err(AppError::Conflict("手机号已存在".into()));
        }
    }

    // Update user profile
    let row = sqlx::query_as::<_, UserRow>(
        r#"UPDATE users SET
            display_name = COALESCE($1, display_name),
            email = COALESCE($2, email),
            phone = COALESCE($3, phone),
            avatar_url = COALESCE($4, avatar_url),
            self_intro = COALESCE($5, self_intro),
            updated_at = now(),
            updated_by = $6
          WHERE id = $7 AND deleted_at IS NULL
          RETURNING *"#,
    )
    .bind(&payload.display_name)
    .bind(&payload.email)
    .bind(&payload.phone)
    .bind(&payload.avatar_url)
    .bind(&payload.self_intro)
    .bind(claims.sub)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::not_found("用户"))?;

    // Update roles if provided
    if let Some(ref role_ids) = payload.role_ids {
        sqlx::query("DELETE FROM user_roles WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        for role_id in role_ids {
            sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2)")
                .bind(user_id)
                .bind(role_id)
                .execute(&mut *tx)
                .await?;
        }
    }

    tx.commit().await?;

    Ok(ApiResponse::success("更新成功", Some(row.into())))
}

/// `DELETE /api/v1/users/:id`
///
/// Soft-deletes a user. Requires `user:manage` permission.
/// Scope: admin can only delete users with "user" role.
pub async fn delete_user(
    State(db): State<PgPool>,
    AuthUser(claims): AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    claims.require_permission("user:manage")?;

    // Cannot delete self
    if user_id == claims.sub {
        return Err(AppError::Forbidden);
    }

    // Single transaction for scope check + soft delete
    let mut tx = db.begin().await?;

    // Scope check (inside transaction to avoid TOCTOU)
    check_manage_scope(&mut *tx, &claims, user_id).await?;

    let result = sqlx::query(
        "UPDATE users SET deleted_at = now(), deleted_by = $1
         WHERE id = $2 AND deleted_at IS NULL",
    )
    .bind(claims.sub)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("用户"));
    }

    tx.commit().await?;

    Ok(ApiResponse::success("删除成功", None))
}
