//! Application error types.
//!
//! Defines [`AppError`] — a unified error enum that implements
//! [`axum::response::IntoResponse`], so handlers can return
//! `Result<ApiResponse<T>, AppError>` and axum converts errors
//! into proper JSON error responses automatically.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use validator::ValidationErrors;

use crate::response::ApiResponse;

/// Unified application error.
///
/// Each variant maps to an HTTP status code and a human-readable
/// message. The `IntoResponse` implementation produces a JSON body
/// consistent with [`ApiResponse`].
#[derive(Debug)]
pub enum AppError {
    /// Malformed request input (invalid cursor, bad parameters).
    BadRequest(String),
    /// Authentication required (no token, malformed token).
    Unauthorized,
    /// Valid credentials but insufficient permissions.
    Forbidden,
    /// Requested resource does not exist.
    NotFound(String),
    /// Resource conflict (e.g., duplicate username / email / phone).
    Conflict(String),
    /// Request body validation failed.
    Validation(ValidationErrors),
    /// Unexpected internal error (logged separately).
    Internal(anyhow::Error),
}

impl AppError {
    /// Convenience constructor for 400 with a detail message.
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    /// Convenience constructor for 404 with a resource name.
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::NotFound(resource.into())
    }

    /// Convenience constructor for 409 with a detail message.
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "未认证或 token 无效".to_string()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "无权限执行此操作".to_string()),
            AppError::NotFound(resource) => (StatusCode::NOT_FOUND, format!("{} 不存在", resource)),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            AppError::Validation(errors) => (StatusCode::UNPROCESSABLE_ENTITY, errors.to_string()),
            AppError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "服务器内部错误".to_string(),
            ),
        };

        let body: ApiResponse<()> = ApiResponse::failure(status, message);
        (status, axum::Json(body)).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        // Map unique-constraint violations (PostgreSQL code 23505) to
        // user-friendly Conflict responses.  We parse the constraint name
        // from the error rather than exposing the raw database message
        // (which leaks table/column names).
        if let sqlx::Error::Database(db_err) = &e
            && db_err.code().as_deref() == Some("23505")
        {
            let constraint = db_err.constraint().unwrap_or("unknown");

            let msg = match constraint {
                "idx_users_username" => "用户名已被占用".to_string(),
                "idx_users_email" => "邮箱已被注册".to_string(),
                "idx_users_phone" => "手机号已被注册".to_string(),
                "idx_roles_name" => "角色名称已存在".to_string(),
                "idx_permissions_resource_action" => "该资源的此操作权限已存在".to_string(),
                _ => "资源已存在，请检查唯一字段".to_string(),
            };
            return Self::Conflict(msg);
        }
        Self::Internal(e.into())
    }
}
