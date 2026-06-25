//! Authentication routes: register and login.

use axum::extract::State;
use axum::Json;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use validator::Validate;

use crate::errors::AppError;
use crate::models::auth::{LoginPayload, RegisterPayload, TokenResponse};
use crate::response::ApiResponse;

/// `POST /api/v1/auth/register`
///
/// Creates a new user with the default "user" role.
/// Returns a JWT token on success.
pub async fn register(
    State(db): State<PgPool>,
    State(config): State<crate::config::SharedConfig>,
    Json(payload): Json<RegisterPayload>,
) -> Result<ApiResponse<TokenResponse>, AppError> {
    // Validate payload
    payload
        .validate()
        .map_err(AppError::Validation)?;

    // Hash password
    let password_hash =
        crate::auth::password::hash_password(&payload.password).map_err(AppError::Internal)?;

    // Begin transaction
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
    let user_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (display_name, username, password_hash, email, phone, avatar_url, self_intro)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id",
    )
    .bind(&payload.display_name)
    .bind(&payload.username)
    .bind(&password_hash)
    .bind(&payload.email)
    .bind(&payload.phone)
    .bind(&payload.avatar_url)
    .bind(&payload.self_intro)
    .fetch_one(&mut *tx)
    .await?;

    // Assign "user" role
    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id)
         SELECT $1, id FROM roles WHERE name = 'user'",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // Load roles & permissions for the new user
    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT r.name FROM roles r
         JOIN user_roles ur ON ur.role_id = r.id
         WHERE ur.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(&db)
    .await?;

    let permissions: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT p.resource || ':' || p.action
         FROM permissions p
         JOIN role_permissions rp ON rp.permission_id = p.id
         JOIN user_roles ur ON ur.role_id = rp.role_id
         WHERE ur.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(&db)
    .await?;

    // Create JWT
    let token = crate::auth::jwt::create_token(
        &config,
        user_id,
        &payload.username,
        roles,
        permissions,
    )
    .map_err(AppError::Internal)?;

    let expires_at = Utc::now()
        + Duration::hours(config.jwt.expiry_hours as i64);

    Ok(ApiResponse::success(
        "注册成功",
        Some(TokenResponse { token, expires_at }),
    ))
}

/// `POST /api/v1/auth/login`
///
/// Verifies credentials and returns a JWT token.
pub async fn login(
    State(db): State<PgPool>,
    State(config): State<crate::config::SharedConfig>,
    Json(payload): Json<LoginPayload>,
) -> Result<ApiResponse<TokenResponse>, AppError> {
    payload
        .validate()
        .map_err(AppError::Validation)?;

    // Look up user
    let user: Option<(uuid::Uuid, String, String)> = sqlx::query_as(
        "SELECT id, username, password_hash FROM users
         WHERE username = $1 AND deleted_at IS NULL",
    )
    .bind(&payload.username)
    .fetch_optional(&db)
    .await?;

    let (user_id, username, password_hash) =
        user.ok_or_else(|| AppError::Unauthorized)?;

    // Verify password
    let valid = crate::auth::password::verify_password(&payload.password, &password_hash)
        .map_err(AppError::Internal)?;

    if !valid {
        return Err(AppError::Unauthorized);
    }

    // Load roles & permissions
    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT r.name FROM roles r
         JOIN user_roles ur ON ur.role_id = r.id
         WHERE ur.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(&db)
    .await?;

    let permissions: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT p.resource || ':' || p.action
         FROM permissions p
         JOIN role_permissions rp ON rp.permission_id = p.id
         JOIN user_roles ur ON ur.role_id = rp.role_id
         WHERE ur.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(&db)
    .await?;

    // Create JWT
    let token = crate::auth::jwt::create_token(
        &config, user_id, &username, roles, permissions,
    )
    .map_err(AppError::Internal)?;

    let expires_at = Utc::now()
        + Duration::hours(config.jwt.expiry_hours as i64);

    Ok(ApiResponse::success(
        "登录成功",
        Some(TokenResponse { token, expires_at }),
    ))
}
