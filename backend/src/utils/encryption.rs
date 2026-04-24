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

/// Encrypt raw bytes with an Additional Authenticated Data (AAD)
/// context string. The AAD isn't ciphertext, but it's bound into the
/// auth tag: decrypting with a different AAD fails. Use this to
/// domain-separate ciphertexts that share the master key (e.g. the
/// plugin local signing key vs MFA secrets) so a blob from one table
/// cannot be swapped into another and still decrypt successfully.
///
/// Returns raw bytes in the format `<12-byte nonce><ciphertext><tag>`,
/// intended for BYTEA columns. The plaintext-bytes variant avoids the
/// base64/hex shuffle the string form does.
pub fn encrypt_bytes_with_aad(plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let key_bytes = get_encryption_key()?;
    let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes)
        .map_err(|_| anyhow!("Failed to create encryption key"))?;
    let sealing_key = LessSafeKey::new(unbound_key);

    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; 12];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| anyhow!("Failed to generate nonce"))?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    let mut in_out = plaintext.to_vec();
    sealing_key
        .seal_in_place_append_tag(nonce, Aad::from(aad), &mut in_out)
        .map_err(|_| anyhow!("Encryption failed"))?;

    let mut result = nonce_bytes.to_vec();
    result.extend_from_slice(&in_out);
    Ok(result)
}

/// Decrypt bytes produced by `encrypt_bytes_with_aad`. The caller
/// must pass the same AAD context that was used at encryption time;
/// any mismatch fails the auth tag check.
pub fn decrypt_bytes_with_aad(ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let key_bytes = get_encryption_key()?;
    let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes)
        .map_err(|_| anyhow!("Failed to create decryption key"))?;
    let opening_key = LessSafeKey::new(unbound_key);

    if ciphertext.len() < 12 + 16 {
        return Err(anyhow!("Encrypted data too short"));
    }

    let (nonce_bytes, body) = ciphertext.split_at(12);
    let nonce =
        Nonce::try_assume_unique_for_key(nonce_bytes).map_err(|_| anyhow!("Invalid nonce"))?;

    let mut in_out = body.to_vec();
    let plaintext = opening_key
        .open_in_place(nonce, Aad::from(aad), &mut in_out)
        .map_err(|_| anyhow!("Decryption failed - invalid key, AAD context, or corrupted data"))?;

    Ok(plaintext.to_vec())
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
    fn aad_roundtrip_and_cross_context_rejection() {
        std::env::set_var(
            "ENCRYPTION_KEY",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        );

        let plaintext = b"pkcs8-private-key-bytes";
        let ctx_a = b"nosdesk.plugin.local_sk.v1";
        let ctx_b = b"nosdesk.mfa.totp.v1";

        let ct = encrypt_bytes_with_aad(plaintext, ctx_a).expect("encrypt with aad a");
        assert_eq!(
            decrypt_bytes_with_aad(&ct, ctx_a).expect("decrypt with aad a"),
            plaintext
        );

        // Wrong AAD context must fail the auth tag check. This is
        // the property that stops ciphertext swapping between tables
        // sharing the master key.
        assert!(decrypt_bytes_with_aad(&ct, ctx_b).is_err());
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
