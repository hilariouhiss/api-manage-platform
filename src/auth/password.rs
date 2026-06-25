//! Password hashing and verification using argon2.

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};

/// Hash a plaintext password with argon2.
///
/// Returns the PHC-encoded hash string (includes salt + params).
pub fn hash_password(password: &str) -> Result<String, anyhow::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))?;
    Ok(hash.to_string())
}

/// Verify a plaintext password against a PHC-encoded hash.
///
/// Returns `Ok(true)` on match, `Ok(false)` on mismatch, `Err` on
/// malformed hash.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, anyhow::Error> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| anyhow::anyhow!("invalid password hash format: {e}"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_success() {
        let password = "Test123!@#";
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash).unwrap());
    }

    #[test]
    fn wrong_password_fails() {
        let hash = hash_password("correct1!").unwrap();
        assert!(!verify_password("wrong2@", &hash).unwrap());
    }
}
