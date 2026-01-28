//! Encryption utilities for sensitive data at rest
//!
//! Uses AES-256-GCM for authenticated encryption.
//! Requires ENCRYPTION_KEY or MFA_ENCRYPTION_KEY environment variable (64 hex chars = 32 bytes).

use anyhow::{anyhow, Result};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};

/// Get encryption key from environment (must be 32 bytes for AES-256-GCM)
/// Checks ENCRYPTION_KEY first, falls back to MFA_ENCRYPTION_KEY for compatibility
fn get_encryption_key() -> Result<[u8; 32]> {
    let key_hex = std::env::var("ENCRYPTION_KEY")
        .or_else(|_| std::env::var("MFA_ENCRYPTION_KEY"))
        .map_err(|_| anyhow!("ENCRYPTION_KEY or MFA_ENCRYPTION_KEY environment variable not set"))?;

    if key_hex.len() != 64 {
        return Err(anyhow!(
            "Encryption key must be exactly 64 hex characters (32 bytes)"
        ));
    }

    let mut key = [0u8; 32];
    hex::decode_to_slice(&key_hex, &mut key)
        .map_err(|_| anyhow!("Encryption key must be valid hexadecimal"))?;

    Ok(key)
}

/// Check if encryption is available (key is configured)
#[allow(dead_code)]
pub fn is_encryption_available() -> bool {
    get_encryption_key().is_ok()
}

/// Encrypt a string using AES-256-GCM
///
/// Returns hex-encoded ciphertext with prepended nonce.
/// Format: <12-byte nonce><ciphertext><16-byte auth tag>
pub fn encrypt(plaintext: &str) -> Result<String> {
    let key_bytes = get_encryption_key()?;
    let unbound_key =
        UnboundKey::new(&AES_256_GCM, &key_bytes).map_err(|_| anyhow!("Failed to create encryption key"))?;
    let sealing_key = LessSafeKey::new(unbound_key);

    // Generate random 12-byte nonce
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; 12];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| anyhow!("Failed to generate nonce"))?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    // Encrypt the plaintext
    let mut in_out = plaintext.as_bytes().to_vec();
    sealing_key
        .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| anyhow!("Encryption failed"))?;

    // Combine nonce + ciphertext and encode as hex
    let mut result = nonce_bytes.to_vec();
    result.extend_from_slice(&in_out);
    Ok(hex::encode(result))
}

/// Decrypt a hex-encoded ciphertext using AES-256-GCM
///
/// Expects format: <12-byte nonce><ciphertext><16-byte auth tag>
pub fn decrypt(encrypted_hex: &str) -> Result<String> {
    let key_bytes = get_encryption_key()?;
    let unbound_key =
        UnboundKey::new(&AES_256_GCM, &key_bytes).map_err(|_| anyhow!("Failed to create decryption key"))?;
    let opening_key = LessSafeKey::new(unbound_key);

    // Decode from hex
    let encrypted_data =
        hex::decode(encrypted_hex).map_err(|_| anyhow!("Invalid encrypted data format"))?;

    if encrypted_data.len() < 12 + 16 {
        // nonce + minimum auth tag
        return Err(anyhow!("Encrypted data too short"));
    }

    // Split nonce and ciphertext
    let (nonce_bytes, ciphertext) = encrypted_data.split_at(12);
    let nonce =
        Nonce::try_assume_unique_for_key(nonce_bytes).map_err(|_| anyhow!("Invalid nonce"))?;

    // Decrypt
    let mut in_out = ciphertext.to_vec();
    let plaintext = opening_key
        .open_in_place(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| anyhow!("Decryption failed - invalid key or corrupted data"))?;

    String::from_utf8(plaintext.to_vec()).map_err(|_| anyhow!("Invalid UTF-8 in decrypted data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        // Set a test key
        std::env::set_var(
            "ENCRYPTION_KEY",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        );

        let original = "my-secret-token-12345";
        let encrypted = encrypt(original).expect("Encryption failed");

        // Encrypted should be different from original
        assert_ne!(encrypted, original);

        // Should be hex-encoded
        assert!(encrypted.chars().all(|c| c.is_ascii_hexdigit()));

        // Decrypt should return original
        let decrypted = decrypt(&encrypted).expect("Decryption failed");
        assert_eq!(decrypted, original);
    }

    #[test]
    fn test_different_encryptions_produce_different_ciphertext() {
        std::env::set_var(
            "ENCRYPTION_KEY",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        );

        let original = "test-secret";
        let encrypted1 = encrypt(original).expect("Encryption 1 failed");
        let encrypted2 = encrypt(original).expect("Encryption 2 failed");

        // Due to random nonce, each encryption should produce different ciphertext
        assert_ne!(encrypted1, encrypted2);

        // But both should decrypt to the same value
        assert_eq!(
            decrypt(&encrypted1).unwrap(),
            decrypt(&encrypted2).unwrap()
        );
    }
}
