use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{AppError, Result};

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| AppError::Security(format!("failed to hash password: {error}")))
}

pub fn verify_password(password_hash: &str, password: &str) -> Result<bool> {
    let parsed = PasswordHash::new(password_hash)
        .map_err(|error| AppError::Security(format!("invalid password hash: {error}")))?;

    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

pub fn new_session_token() -> String {
    Uuid::new_v4().to_string()
}

pub fn hash_session_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}
