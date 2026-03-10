use std::env;

use aes_gcm_siv::{
    aead::{Aead, KeyInit},
    Aes256GcmSiv, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

use crate::error::{AppError, Result};

const MASTER_KEY_ENV: &str = "WELLBEING_MASTER_KEY";

pub fn encrypt_user_secret(value: &str) -> Result<String> {
    let cipher = cipher_from_env()?;
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, value.as_bytes())
        .map_err(|_| AppError::Security("failed to encrypt user secret".to_string()))?;

    let mut payload = nonce_bytes.to_vec();
    payload.extend(ciphertext);
    Ok(STANDARD.encode(payload))
}

pub fn decrypt_user_secret(value: &str) -> Result<String> {
    let cipher = cipher_from_env()?;
    let payload = STANDARD
        .decode(value)
        .map_err(|_| AppError::Security("stored user secret is not valid base64".to_string()))?;
    if payload.len() < 13 {
        return Err(AppError::Security(
            "stored user secret is too short to decrypt".to_string(),
        ));
    }

    let (nonce_bytes, ciphertext) = payload.split_at(12);
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| AppError::Security("failed to decrypt user secret".to_string()))?;
    String::from_utf8(plaintext)
        .map_err(|_| AppError::Security("decrypted user secret was not valid utf-8".to_string()))
}

fn cipher_from_env() -> Result<Aes256GcmSiv> {
    let master = env::var(MASTER_KEY_ENV).map_err(|_| {
        AppError::InvalidState(format!(
            "{MASTER_KEY_ENV} must be set before personal provider keys can be stored"
        ))
    })?;

    let digest = Sha256::digest(master.as_bytes());
    Aes256GcmSiv::new_from_slice(&digest)
        .map_err(|_| AppError::Security("failed to derive cipher key".to_string()))
}
