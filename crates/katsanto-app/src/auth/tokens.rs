//! Random secrets (session tokens, invite/reset links, recovery codes).
//! Only SHA-256 hashes are stored; secrets are high-entropy, so no slow hash is needed.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{Rng, RngCore};
use sha2::{Digest, Sha256};

/// 256-bit random token, base64url.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn hash(secret: &str) -> Vec<u8> {
    Sha256::digest(secret.as_bytes()).to_vec()
}

pub const RECOVERY_CODE_COUNT: usize = 10;
/// No 0/O/1/I/L to keep codes readable.
const CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";

/// Recovery code like `7KQ2-MX9D-HT4W` (~59 bits).
pub fn new_recovery_code() -> String {
    let mut rng = rand::rng();
    let chars: String = (0..12)
        .map(|_| CODE_ALPHABET[rng.random_range(0..CODE_ALPHABET.len())] as char)
        .collect();
    format!("{}-{}-{}", &chars[..4], &chars[4..8], &chars[8..])
}

pub fn new_recovery_codes() -> Vec<String> {
    (0..RECOVERY_CODE_COUNT)
        .map(|_| new_recovery_code())
        .collect()
}

/// Hash a recovery code as typed by the user: case and separators don't matter.
pub fn hash_recovery_code(code: &str) -> Vec<u8> {
    let normalized: String = code
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    hash(&normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_and_urlsafe() {
        let (a, b) = (new_token(), new_token());
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn recovery_codes_normalize() {
        let code = new_recovery_code();
        assert_eq!(code.len(), 14);
        assert_eq!(
            hash_recovery_code(&code),
            hash_recovery_code(&code.to_lowercase())
        );
        assert_eq!(
            hash_recovery_code(&code),
            hash_recovery_code(&format!(" {} ", code.replace('-', " ")))
        );
        assert_ne!(
            hash_recovery_code(&code),
            hash_recovery_code(&new_recovery_code())
        );
    }
}
