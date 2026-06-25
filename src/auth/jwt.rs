//! JWT token creation and validation.

use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use uuid::Uuid;

use crate::config::SharedConfig;
use crate::models::auth::JwtClaims;

/// Create a signed JWT access token for the given user.
///
/// Returns the encoded token string.
pub fn create_token(
    config: &SharedConfig,
    user_id: Uuid,
    username: &str,
    roles: Vec<String>,
    permissions: Vec<String>,
) -> Result<String, anyhow::Error> {
    let now = Utc::now();
    let expiry_hours = config.jwt.expiry_hours as i64;
    let exp = (now + Duration::hours(expiry_hours)).timestamp() as usize;

    let claims = JwtClaims {
        sub: user_id,
        username: username.to_string(),
        roles,
        permissions,
        exp,
    };

    let key = EncodingKey::from_secret(config.jwt.secret.as_bytes());
    let token = encode(&Header::default(), &claims, &key)
        .map_err(|e| anyhow::anyhow!("failed to create JWT: {e}"))?;

    Ok(token)
}

/// Validate and decode a JWT token.
///
/// Returns the claims on success, or an error if the token is
/// expired, malformed, or has an invalid signature.
pub fn verify_token(config: &SharedConfig, token: &str) -> Result<JwtClaims, anyhow::Error> {
    let key = DecodingKey::from_secret(config.jwt.secret.as_bytes());
    let validation = Validation::default();
    let data = decode::<JwtClaims>(token, &key, &validation)
        .map_err(|e| anyhow::anyhow!("invalid JWT token: {e}"))?;
    Ok(data.claims)
}
