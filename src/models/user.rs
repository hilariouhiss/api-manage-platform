//! User-related types: database rows, request/response payloads,
//! and pagination helpers.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ── Database row ──────────────────────────────────────────────

/// A full row from the `users` table.
///
/// **Never** returned directly via the API — use [`UserResponse`]
/// or [`UserListItem`] which exclude `password_hash`.
#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub display_name: String,
    pub username: String,
    pub password_hash: String,
    pub email: String,
    pub phone: String,
    pub avatar_url: Option<String>,
    pub self_intro: Option<String>,
    pub created_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub updated_at: Option<DateTime<Utc>>,
    pub updated_by: Option<Uuid>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub deleted_by: Option<Uuid>,
}

// ── API response types ────────────────────────────────────────

/// Public user profile — excludes `password_hash` and audit fields.
#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub display_name: String,
    pub username: String,
    pub email: String,
    pub phone: String,
    pub avatar_url: Option<String>,
    pub self_intro: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl From<UserRow> for UserResponse {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            display_name: row.display_name,
            username: row.username,
            email: row.email,
            phone: row.phone,
            avatar_url: row.avatar_url,
            self_intro: row.self_intro,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Compact user info for list responses.
#[derive(Debug, Serialize)]
pub struct UserListItem {
    pub id: Uuid,
    pub display_name: String,
    pub username: String,
    pub email: String,
    pub phone: String,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<UserRow> for UserListItem {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            display_name: row.display_name,
            username: row.username,
            email: row.email,
            phone: row.phone,
            avatar_url: row.avatar_url,
            created_at: row.created_at,
        }
    }
}

// ── Request payloads ──────────────────────────────────────────

/// POST /api/v1/users (admin creates a user)
#[derive(Debug, Deserialize, validator::Validate)]
pub struct CreateUserPayload {
    #[validate(length(
        min = 1,
        max = 100,
        message = "展示名不能为空，最多 100 个字符"
    ))]
    pub display_name: String,

    #[validate(length(min = 3, max = 50, message = "用户名长度需在 3-50 之间"))]
    pub username: String,

    #[validate(custom(function = "super::auth::validate_password"))]
    pub password: String,

    #[validate(email(message = "邮箱格式不正确"))]
    pub email: String,

    #[validate(length(min = 1, max = 30, message = "手机号不能为空，最多 30 位"))]
    pub phone: String,

    pub avatar_url: Option<String>,
    pub self_intro: Option<String>,
}

/// PUT /api/v1/users/me or PUT /api/v1/users/:id
#[derive(Debug, Deserialize, validator::Validate)]
pub struct UpdateUserPayload {
    #[validate(length(
        min = 1,
        max = 100,
        message = "展示名不能为空，最多 100 个字符"
    ))]
    pub display_name: Option<String>,

    #[validate(email(message = "邮箱格式不正确"))]
    pub email: Option<String>,

    #[validate(length(min = 1, max = 30, message = "手机号最多 30 位"))]
    pub phone: Option<String>,

    pub avatar_url: Option<String>,
    pub self_intro: Option<String>,

    /// Only settable by admin — assign roles to the user.
    pub role_ids: Option<Vec<Uuid>>,
}

// ── Pagination types ──────────────────────────────────────────

/// Cursor payload encoded in base64url for keyset pagination.
#[derive(Debug, Serialize, Deserialize)]
pub struct UserCursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

impl UserCursor {
    /// Encode cursor to a base64url string.
    pub fn encode(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        URL_SAFE.encode(json.as_bytes())
    }

    /// Decode cursor from a base64url string.
    pub fn decode(raw: &str) -> Result<Self, crate::errors::AppError> {
        let bytes = URL_SAFE
            .decode(raw)
            .map_err(|_| crate::errors::AppError::bad_request("无效的分页游标"))?;
        let json = String::from_utf8(bytes)
            .map_err(|_| crate::errors::AppError::bad_request("无效的分页游标"))?;
        serde_json::from_str(&json)
            .map_err(|_| crate::errors::AppError::bad_request("无效的分页游标"))
    }
}

/// Paginated response wrapper for keyset (cursor) pagination.
#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    /// Opaque cursor for the next page (absent when `has_more` is false).
    pub next_cursor: Option<String>,
    /// Whether more items exist after this page.
    pub has_more: bool,
}
