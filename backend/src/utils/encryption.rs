//! Encryption utilities for sensitive data at rest
//!
//! Uses AES-256-GCM for authenticated encryption.
//! Requires ENCRYPTION_KEY or MFA_ENCRYPTION_KEY environment variable (64 hex chars = 32 bytes).

use anyhow::{anyhow, Result};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};

/// Errors a loaded KEK can fail with at boot. The variants are
/// distinct so `main.rs` can apply different prod/dev policy per
/// failure mode — e.g. `LooksLikeAscii` warns in dev but hard-fails
/// in prod, whereas `AllZero` and `ConstantByte` hard-fail in both
/// because they have zero false-positive surface.
///
/// We *intentionally* don't add a Shannon-entropy floor: with only
/// 32 bytes of sample, Shannon is a poor estimator (Kaminsky et al.
/// 2021) and will either be too lax to catch the realistic
/// misconfiguration or too strict and reject legitimate
/// `openssl rand -hex 32` output. The cheap byte-pattern checks
/// below catch every footgun the entropy check would catch, with
/// no false-positive risk.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("ENCRYPTION_KEY or MFA_ENCRYPTION_KEY environment variable not set")]
    NotSet,
    #[error("Encryption key must be exactly 64 hex characters (32 bytes)")]
    BadLength,
    #[error("Encryption key must be valid hexadecimal")]
    BadHex,
    #[error("Encryption key is all zeros; refusing to use a non-key. Generate one with: openssl rand -hex 32")]
    AllZero,
    #[error("Encryption key has no entropy (all bytes are 0x{0:02x}); refusing. Generate one with: openssl rand -hex 32")]
    ConstantByte(u8),
    #[error("Encryption key looks like ASCII text, not random bytes. Did you paste a passphrase? Generate one with: openssl rand -hex 32")]
    LooksLikeAscii,
}

/// Reject obvious operator-footgun keys. These checks are cheap
/// (O(32)) and have zero false-positive surface for real
/// `openssl rand -hex 32` output:
///
///   - all-zero: the canonical "I left the .env.example placeholder
///     in production" failure mode
///   - constant-byte (e.g. `0x42` repeated 32 times): same family
///     as all-zero, separately reported so the operator gets a
///     precise diagnostic
///   - all-printable-ASCII: catches "I pasted a passphrase that
///     happened to be 64 hex chars long" — vanishingly rare given
///     the prior hex-decode would catch most passphrases, but
///     free to add
///
/// Deliberately NOT included: repeating-pattern detection beyond
/// constant-byte (vanishingly rare in real world), Shannon entropy
/// (bad estimator on 32 bytes), KCV/dictionary checks (overkill
/// for this surface). See the file-header comment on `KeyError`
/// for the reasoning.
fn validate_key_material(key: &[u8; 32]) -> Result<(), KeyError> {
    if key.iter().all(|&b| b == 0) {
        return Err(KeyError::AllZero);
    }
    if key.iter().all(|&b| b == key[0]) {
        return Err(KeyError::ConstantByte(key[0]));
    }
    // 0x20..=0x7e is printable ASCII (space through tilde). If all
    // 32 bytes fall in this range, the input was almost certainly
    // a passphrase, not random bytes.
    if key.iter().all(|&b| (0x20..=0x7e).contains(&b)) {
        return Err(KeyError::LooksLikeAscii);
    }
    Ok(())
}

/// Get encryption key from environment (must be 32 bytes for AES-256-GCM)
/// Checks ENCRYPTION_KEY first, falls back to MFA_ENCRYPTION_KEY for compatibility
fn get_encryption_key() -> Result<[u8; 32]> {
    load_encryption_key().map_err(|e| anyhow!(e.to_string()))
}

/// Lower-level variant of [`get_encryption_key`] that surfaces the
/// typed [`KeyError`] so the boot path in `main.rs` can apply
/// prod-vs-dev policy per failure mode. The `anyhow`-erased
/// variant is what every other in-app call site uses — they only
/// care that *some* error happened, not which one.
pub fn load_encryption_key() -> Result<[u8; 32], KeyError> {
    let key_hex = std::env::var("ENCRYPTION_KEY")
        .or_else(|_| std::env::var("MFA_ENCRYPTION_KEY"))
        .map_err(|_| KeyError::NotSet)?;

    if key_hex.len() != 64 {
        return Err(KeyError::BadLength);
    }

    let mut key = [0u8; 32];
    hex::decode_to_slice(&key_hex, &mut key).map_err(|_| KeyError::BadHex)?;

    validate_key_material(&key)?;
    Ok(key)
}

/// Validate the encryption key configuration at startup. Call from
/// `main()` after env load — surfaces a misconfigured key as a boot
/// failure instead of a 500 the first time MFA / API tokens / SLA
/// secrets need decryption mid-request. The "user is locked out
/// of MFA setup three days after deploy because the env var was
/// truncated" failure mode is far worse than refusing to boot.
///
/// Use [`load_encryption_key`] directly in `main.rs` if you need
/// to distinguish failure modes for the dev/prod policy split.
pub fn validate_at_startup() -> Result<()> {
    let _ = get_encryption_key()?;
    Ok(())
}

#[cfg(test)]
mod key_validation_tests {
    use super::*;

    #[test]
    fn all_zero_rejected() {
        assert!(matches!(
            validate_key_material(&[0u8; 32]),
            Err(KeyError::AllZero)
        ));
    }

    #[test]
    fn constant_byte_rejected() {
        let k = [0x42u8; 32];
        assert!(matches!(
            validate_key_material(&k),
            Err(KeyError::ConstantByte(0x42))
        ));
    }

    #[test]
    fn ascii_passphrase_rejected() {
        // 64-char ASCII passphrase decoded as 32 bytes would land
        // entirely in the printable range.
        let mut k = [0u8; 32];
        for (i, b) in b"this is a very long but not-random passphrase!  ".iter().take(32).enumerate() {
            k[i] = *b;
        }
        assert!(matches!(
            validate_key_material(&k),
            Err(KeyError::LooksLikeAscii)
        ));
    }

    #[test]
    fn real_random_key_accepted() {
        // Sample of `openssl rand -hex 32` output, hex-decoded.
        // Has both control bytes and high bytes, fails all three
        // footgun checks (correctly).
        let k = [
            0x9f, 0x2c, 0xa8, 0x01, 0x4d, 0xe7, 0xfb, 0x33,
            0x18, 0xb5, 0x60, 0x9c, 0x7a, 0x21, 0xd4, 0x88,
            0xe3, 0x4f, 0x96, 0x2b, 0xc0, 0x55, 0x71, 0x0d,
            0x84, 0xa9, 0x12, 0x67, 0xeb, 0x3a, 0xcc, 0xf5,
        ];
        assert!(validate_key_material(&k).is_ok());
    }

    #[test]
    fn near_all_zero_with_one_nonzero_byte_accepted() {
        // Defensive: only EXACTLY all-zero gets the AllZero diagnostic;
        // any other low-entropy-but-not-constant key passes the
        // current checks. This is intentional — we don't have a
        // good universal "low entropy" detector that doesn't
        // false-positive on real random output.
        let mut k = [0u8; 32];
        k[31] = 1;
        assert!(validate_key_material(&k).is_ok());
    }
}

/// Encrypt a string using AES-256-GCM
///
/// Returns hex-encoded ciphertext with prepended nonce.
/// Format: <12-byte nonce><ciphertext><16-byte auth tag>
pub fn encrypt(plaintext: &str) -> Result<String> {
    let key_bytes = get_encryption_key()?;
    let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes)
        .map_err(|_| anyhow!("Failed to create encryption key"))?;
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
///
/// Returns `Zeroizing<Vec<u8>>` so the plaintext is zeroed when
/// dropped. Callers who need to hold the bytes longer should keep
/// the `Zeroizing` wrapper; dereferencing to `&[u8]` is cheap.
pub fn decrypt_bytes_with_aad(
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    use zeroize::Zeroizing;

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

    // `in_out` holds the decrypted plaintext in place after
    // `open_in_place`. Wrapping in Zeroizing means any panic or
    // early return past this point still wipes the buffer before
    // the allocator reclaims the pages.
    let mut in_out: Zeroizing<Vec<u8>> = Zeroizing::new(body.to_vec());
    let plaintext_len = opening_key
        .open_in_place(nonce, Aad::from(aad), &mut in_out)
        .map_err(|_| anyhow!("Decryption failed - invalid key, AAD context, or corrupted data"))?
        .len();
    in_out.truncate(plaintext_len);
    Ok(in_out)
}

/// Decrypt a hex-encoded ciphertext using AES-256-GCM
///
/// Expects format: <12-byte nonce><ciphertext><16-byte auth tag>
pub fn decrypt(encrypted_hex: &str) -> Result<String> {
    let key_bytes = get_encryption_key()?;
    let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes)
        .map_err(|_| anyhow!("Failed to create decryption key"))?;
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
        let decrypted = decrypt_bytes_with_aad(&ct, ctx_a).expect("decrypt with aad a");
        assert_eq!(decrypted.as_slice(), plaintext.as_slice());

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
        assert_eq!(decrypt(&encrypted1).unwrap(), decrypt(&encrypted2).unwrap());
    }
}
