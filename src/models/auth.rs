//! Authentication-related types: JWT claims, login/register payloads,
//! and token responses.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// JWT claims embedded in access tokens.
///
/// Permissions are flattened into the token so that
/// authorization checks require no database round-trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Subject — the user's UUID.
    pub sub: Uuid,
    /// Username for logging / display.
    pub username: String,
    /// Role names (e.g. `["user"]`, `["admin"]`).
    pub roles: Vec<String>,
    /// Flattened permission names (e.g. `["user:read", "user:update"]`).
    pub permissions: Vec<String>,
    /// Expiration timestamp (UTC).
    pub exp: usize,
}

impl JwtClaims {
    /// Check whether these claims contain the given permission.
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.iter().any(|p| p == permission)
    }

    /// Require a permission, returning [`AppError::Forbidden`] if absent.
    pub fn require_permission(&self, permission: &str) -> Result<(), crate::errors::AppError> {
        if self.has_permission(permission) {
            Ok(())
        } else {
            Err(crate::errors::AppError::Forbidden)
        }
    }

    /// Check whether the user has a specific role.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

// ── Request payloads ──────────────────────────────────────────

/// POST /api/v1/auth/register
#[derive(Debug, Deserialize, validator::Validate)]
pub struct RegisterPayload {
    #[validate(length(
        min = 1,
        max = 100,
        message = "展示名不能为空，最多 100 个字符"
    ))]
    pub display_name: String,

    #[validate(length(min = 3, max = 50, message = "用户名长度需在 3-50 之间"))]
    pub username: String,

    #[validate(custom(function = "validate_password"))]
    pub password: String,

    #[validate(email(message = "邮箱格式不正确"))]
    pub email: String,

    #[validate(length(min = 1, max = 30, message = "手机号不能为空，最多 30 位"))]
    pub phone: String,

    pub avatar_url: Option<String>,
    pub self_intro: Option<String>,
}

/// POST /api/v1/auth/login
#[derive(Debug, Deserialize, validator::Validate)]
pub struct LoginPayload {
    #[validate(length(min = 1, message = "用户名不能为空"))]
    pub username: String,

    #[validate(length(min = 1, message = "密码不能为空"))]
    pub password: String,
}

// ── Response payloads ─────────────────────────────────────────

/// Token returned after successful login / register.
#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

/// Password validation: min 8 chars, must include digit + letter + special char.
pub fn validate_password(password: &str) -> Result<(), validator::ValidationError> {
    if password.len() < 8 {
        let mut err = validator::ValidationError::new("password_too_short");
        err.message = Some("密码长度不能少于 8 位".into());
        return Err(err);
    }

    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_alpha = password.chars().any(|c| c.is_ascii_alphabetic());
    let has_special = password
        .chars()
        .any(|c| c.is_ascii() && !c.is_ascii_alphanumeric());

    if !has_digit || !has_alpha || !has_special {
        let mut err = validator::ValidationError::new("password_weak");
        err.message =
            Some("密码必须包含至少一个数字、一个英文字母和一个特殊字符".into());
        return Err(err);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use validator::Validate;

    #[test]
    fn password_too_short() {
        let p = RegisterPayload {
            display_name: "Test".into(),
            username: "test".into(),
            password: "A1!".into(),
            email: "test@example.com".into(),
            phone: "13800138000".into(),
            avatar_url: None,
            self_intro: None,
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn password_missing_digit() {
        let p = RegisterPayload {
            display_name: "Test".into(),
            username: "test".into(),
            password: "Abcdefg!".into(),
            email: "test@example.com".into(),
            phone: "13800138000".into(),
            avatar_url: None,
            self_intro: None,
        };
        let err = p.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("数字"), "expected digit complaint: {msg}");
    }

    #[test]
    fn password_missing_special() {
        let p = RegisterPayload {
            display_name: "Test".into(),
            username: "test".into(),
            password: "Abcdefg1".into(),
            email: "test@example.com".into(),
            phone: "13800138000".into(),
            avatar_url: None,
            self_intro: None,
        };
        let err = p.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("特殊"), "expected special char complaint: {msg}");
    }

    #[test]
    fn password_valid() {
        let p = RegisterPayload {
            display_name: "Test".into(),
            username: "test".into(),
            password: "Abcdef1!".into(),
            email: "test@example.com".into(),
            phone: "13800138000".into(),
            avatar_url: None,
            self_intro: None,
        };
        assert!(p.validate().is_ok());
    }
}
