//! JWT authentication extractor.
//!
//! [`AuthUser`] implements [`axum::extract::FromRequestParts`] so that
//! handlers can extract the authenticated user directly:
//!
//! ```rust,ignore
//! async fn handler(AuthUser(claims): AuthUser) -> ApiResponse<...> { ... }
//! ```

use axum::extract::{FromRef, FromRequestParts, State};
use axum::http::request::Parts;
use axum::response::IntoResponse;

use crate::config::SharedConfig;
use crate::errors::AppError;
use crate::models::auth::JwtClaims;

/// Extractor that authenticates a request via JWT Bearer token.
///
/// On success the inner [`JwtClaims`] are available to the handler.
/// On failure an [`AppError::Unauthorized`] is returned as a 401 JSON
/// response.
#[derive(Debug, Clone)]
pub struct AuthUser(pub JwtClaims);

impl AuthUser {
    /// Check whether the authenticated user has a specific permission.
    pub fn has_permission(&self, permission: &str) -> bool {
        self.0.has_permission(permission)
    }

    /// Require a permission, returning [`AppError::Forbidden`] if absent.
    pub fn require_permission(&self, permission: &str) -> Result<(), AppError> {
        self.0.require_permission(permission)
    }

    /// Check whether the user has a specific role.
    pub fn has_role(&self, role: &str) -> bool {
        self.0.has_role(role)
    }

    /// The authenticated user's id.
    pub fn user_id(&self) -> uuid::Uuid {
        self.0.sub
    }

    /// The authenticated user's username.
    pub fn username(&self) -> &str {
        &self.0.username
    }
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    SharedConfig: FromRef<S>,
{
    type Rejection = axum::response::Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Extract SharedConfig from state via State extractor
        let State(config) = State::<SharedConfig>::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::Unauthorized.into_response())?;

        // Extract Authorization header
        let header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized.into_response())?;

        // Expect "Bearer <token>"
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::Unauthorized.into_response())?;

        // Verify and decode
        let claims = crate::auth::jwt::verify_token(&config, token)
            .map_err(|_| AppError::Unauthorized.into_response())?;

        Ok(AuthUser(claims))
    }
}
