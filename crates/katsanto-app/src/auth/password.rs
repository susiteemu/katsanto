//! Password hashing (argon2id) and policy.

use std::sync::LazyLock;

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use rand::RngCore;

use crate::error::{AppError, AppResult};

pub const MIN_LEN: usize = 12;
pub const MAX_LEN: usize = 256;

/// The most common passwords that pass the length check.
const COMMON: &[&str] = &[
    "123456789012",
    "1234567890123",
    "12345678910",
    "123456789123",
    "qwertyuiop123",
    "password1234",
    "password12345",
    "passwordpassword",
    "iloveyou1234",
    "qwertyqwerty",
    "111111111111",
    "000000000000",
    "123123123123",
    "abcdefghijkl",
    "abc123abc123",
    "qwerty123456",
    "1q2w3e4r5t6y",
    "1qaz2wsx3edc",
    "zaq12wsxcde3",
    "asdfghjkl123",
    "welcome12345",
    "letmein12345",
    "administrator",
    "changeme1234",
    "football1234",
    "baseball1234",
    "superman1234",
    "trustno11234",
    "monkey123456",
    "dragon123456",
    "katsanto1234",
    "katsantokatsanto",
    "youtube12345",
    "youtubeyoutube",
];

/// Hashing an unknown username's password takes as long as a real one.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| hash_blocking("katsanto-dummy-password"));

pub fn validate(password: &str) -> AppResult<()> {
    let len = password.chars().count();
    if len < MIN_LEN {
        return Err(AppError::BadRequest(format!(
            "Password must be at least {MIN_LEN} characters"
        )));
    }
    if len > MAX_LEN {
        return Err(AppError::BadRequest(format!(
            "Password must be at most {MAX_LEN} characters"
        )));
    }
    if COMMON.contains(&password.to_lowercase().as_str()) {
        return Err(AppError::BadRequest(
            "This password is too common, please choose another one".into(),
        ));
    }
    Ok(())
}

fn hash_blocking(password: &str) -> String {
    let mut salt = [0u8; 16];
    rand::rng().fill_bytes(&mut salt);
    let salt = SaltString::encode_b64(&salt).expect("valid salt length");
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("argon2 hashing")
        .to_string()
}

/// argon2 is deliberately slow; keep it off the async workers.
pub async fn hash(password: String) -> AppResult<String> {
    tokio::task::spawn_blocking(move || hash_blocking(&password))
        .await
        .map_err(AppError::internal)
}

/// Verify against `hash`, or against a dummy hash when there is none, so that
/// response times don't reveal which usernames exist.
pub async fn verify(password: String, hash: Option<String>) -> AppResult<bool> {
    tokio::task::spawn_blocking(move || {
        let (stored, real) = match &hash {
            Some(h) => (h.as_str(), true),
            None => (DUMMY_HASH.as_str(), false),
        };
        let ok = PasswordHash::new(stored)
            .map(|parsed| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &parsed)
                    .is_ok()
            })
            .unwrap_or(false);
        ok && real
    })
    .await
    .map_err(AppError::internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy() {
        assert!(validate("short").is_err());
        assert!(
            validate("Password1234").is_err(),
            "common, case-insensitive"
        );
        assert!(validate("correct horse battery").is_ok());
        assert!(validate(&"x".repeat(MAX_LEN + 1)).is_err());
        // Length counts characters, not bytes.
        assert!(validate("ääääääääääää").is_ok());
    }

    #[tokio::test]
    async fn hash_and_verify() {
        let h = hash("correct horse battery".into()).await.unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(
            verify("correct horse battery".into(), Some(h.clone()))
                .await
                .unwrap()
        );
        assert!(!verify("wrong horse battery".into(), Some(h)).await.unwrap());
        assert!(
            !verify("katsanto-dummy-password".into(), None)
                .await
                .unwrap()
        );
    }
}
