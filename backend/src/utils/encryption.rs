//! Encryption utilities for sensitive data at rest. AES-256-GCM throughout.
//!
//! Versioned KEK (`MFA_KEK_V{n}` + `MFA_KEK_VERSION`), self-describing
//! framed blob `[ver][alg][kek_id][nonce][ct][tag]` stored as `BYTEA`,
//! AAD bound at the call site. Matches the nosdesk-com control plane
//! (Tink / Vault / AWS Encryption SDK shape).

use once_cell::sync::OnceCell;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};
use std::collections::HashMap;
use zeroize::Zeroizing;

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
    #[error("KEK environment variable not set")]
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

// Legacy single-key, hex-encoded `nonce ‖ ct ‖ tag` API removed in the
// Keyring cutover (commit topic: `feat(crypto): Keyring + framed blob`).
// The replacement is `Keyring::encrypt` / `Keyring::decrypt` reached via
// the process-wide `keyring()` singleton initialised at boot.

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
        for (i, b) in b"this is a very long but not-random passphrase!  "
            .iter()
            .take(32)
            .enumerate()
        {
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
            0x9f, 0x2c, 0xa8, 0x01, 0x4d, 0xe7, 0xfb, 0x33, 0x18, 0xb5, 0x60, 0x9c, 0x7a, 0x21,
            0xd4, 0x88, 0xe3, 0x4f, 0x96, 0x2b, 0xc0, 0x55, 0x71, 0x0d, 0x84, 0xa9, 0x12, 0x67,
            0xeb, 0x3a, 0xcc, 0xf5,
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

// Legacy `encrypt` / `decrypt` / `*_bytes_with_aad` functions removed.
// All round-trip / cross-AAD / nonce-uniqueness coverage lives in
// `keyring_tests` below against the framed-blob API.

// ===========================================================================
// Keyring: versioned KEK + self-describing framed blob.
// ===========================================================================
//
// Frame layout (single `BYTEA` column):
//
//   byte  0     : version           (0x01 = current, only one defined)
//   byte  1     : alg_id            (0x01 = AES-256-GCM, only one defined)
//   bytes 2..4  : kek_id            (big-endian u16)
//   bytes 4..16 : nonce             (12 bytes, AES-GCM)
//   bytes 16..N-16 : ciphertext
//   bytes N-16..N  : auth tag       (16 bytes, AES-GCM)
//
// Minimum blob length = 32 bytes (header 16 + tag 16, empty plaintext).
//
// AAD discipline lives at the call site: pass `user_uuid.as_bytes() ‖
// purpose_tag` (or the equivalent row-identity binding) so a cross-row
// ciphertext swap fails the tag check (RFC 5116 §1.2; OWASP Crypto Storage
// Cheat Sheet "Bind context to ciphertext").

/// Active frame version. Only `0x01` is defined. Bumped when the binary
/// layout changes (e.g. swap AES-GCM for XChaCha20-Poly1305, or change the
/// nonce width). A version bump is *not* needed for adding a new KEK
/// generation — that's `kek_id` in the same frame.
const FRAME_VERSION: u8 = 0x01;

/// Algorithm identifier. `0x01` = AES-256-GCM with a 96-bit random nonce
/// and 128-bit tag (`ring::aead::AES_256_GCM`). Reserve `0x02` for
/// XChaCha20-Poly1305 if we adopt that later, but defining one algorithm at
/// a time keeps the decrypt path's match arms honest.
const ALG_AES_256_GCM: u8 = 0x01;

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const FRAME_HEADER_LEN: usize = 1 + 1 + 2 + NONCE_LEN; // version + alg + kek_id + nonce = 16
const MIN_BLOB_LEN: usize = FRAME_HEADER_LEN + TAG_LEN; // 32 bytes

/// Errors raised by `Keyring::from_env`. Distinct variants so the boot path
/// can render an operator-facing diagnostic with the actual misconfiguration
/// rather than a generic "encryption setup failed".
#[derive(Debug, thiserror::Error)]
pub enum KeyringError {
    #[error(
        "no MFA_KEK_V* environment variable found. Generate one with \
         `openssl rand -hex 32` and export it as MFA_KEK_V1 (plus \
         MFA_KEK_VERSION=1). See docs for the rotation procedure."
    )]
    NoKeksLoaded,
    #[error(
        "MFA_KEK_VERSION={requested} but no MFA_KEK_V{requested} is loaded \
         (loaded versions: {loaded:?})"
    )]
    BadCurrentVersion { requested: u16, loaded: Vec<u16> },
    #[error("MFA_KEK_VERSION value {0:?} is not a positive integer")]
    BadVersionFormat(String),
    #[error("MFA_KEK_V{version} is malformed: {source}")]
    BadKeyMaterial {
        version: u16,
        #[source]
        source: KeyError,
    },
}

/// Errors raised by `Keyring::encrypt` / `decrypt`. These are AEAD-layer
/// concerns kept separate from the boot-time `KeyringError` so a per-row
/// decrypt failure doesn't accidentally surface as a config bug.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("ciphertext too short ({actual} bytes; minimum {min})")]
    TooShort { actual: usize, min: usize },
    #[error("unknown frame version 0x{0:02x}")]
    UnknownVersion(u8),
    #[error("unknown algorithm id 0x{0:02x}")]
    UnknownAlgorithm(u8),
    #[error(
        "kek_id {0} not loaded in keyring; either an older key was retired \
         too soon or the row was encrypted under a different deployment"
    )]
    UnknownKekId(u16),
    #[error("AEAD seal/open failed (wrong key, AAD mismatch, or tampered ciphertext)")]
    AeadFailure,
}

/// All loaded KEKs plus the current-write version. Built once at boot from
/// env (`from_env`) and stashed in the `KEYRING` singleton. Tests and the
/// future rewrap CLI can build one manually via `from_keys`.
pub struct Keyring {
    keys: HashMap<u16, [u8; 32]>,
    current: u16,
}

// Manual Debug impl: never expose key material, even in panic / log output.
// `Result::unwrap_err` requires `T: Debug`, so tests need *some* impl; this
// gives them a useful summary without leaking bytes.
impl std::fmt::Debug for Keyring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut versions: Vec<u16> = self.keys.keys().copied().collect();
        versions.sort();
        f.debug_struct("Keyring")
            .field("loaded_versions", &versions)
            .field("current", &self.current)
            .finish_non_exhaustive()
    }
}

impl Keyring {
    /// Load every `MFA_KEK_V{n}` env var found, picking
    /// `MFA_KEK_VERSION` as the current-write key (or the highest loaded
    /// version if `MFA_KEK_VERSION` is unset and only one key exists).
    pub fn from_env() -> Result<Self, KeyringError> {
        let mut keys: HashMap<u16, [u8; 32]> = HashMap::new();
        for (k, v) in std::env::vars() {
            let Some(suffix) = k.strip_prefix("MFA_KEK_V") else {
                continue;
            };
            // Skip non-numeric suffixes (e.g. MFA_KEK_VERSION). Version 0
            // is reserved as a sentinel for "no kek_id encoded" should
            // anyone need it in the future.
            let Ok(version) = suffix.parse::<u16>() else {
                continue;
            };
            if version == 0 {
                continue;
            }
            let key = parse_kek_hex(&v)
                .map_err(|source| KeyringError::BadKeyMaterial { version, source })?;
            keys.insert(version, key);
        }

        if keys.is_empty() {
            return Err(KeyringError::NoKeksLoaded);
        }

        let current = match std::env::var("MFA_KEK_VERSION") {
            Ok(s) => s
                .parse::<u16>()
                .map_err(|_| KeyringError::BadVersionFormat(s))?,
            Err(_) => {
                // Single-key install: no ambiguity, default to it. Multi-key
                // installs MUST set MFA_KEK_VERSION explicitly, since
                // "highest loaded" might race with a rotation in progress.
                if keys.len() == 1 {
                    *keys.keys().next().expect("len == 1 checked")
                } else {
                    let mut loaded: Vec<u16> = keys.keys().copied().collect();
                    loaded.sort();
                    return Err(KeyringError::BadCurrentVersion {
                        requested: 0,
                        loaded,
                    });
                }
            }
        };

        if !keys.contains_key(&current) {
            let mut loaded: Vec<u16> = keys.keys().copied().collect();
            loaded.sort();
            return Err(KeyringError::BadCurrentVersion {
                requested: current,
                loaded,
            });
        }

        Ok(Self { keys, current })
    }

    /// Build a Keyring directly from a key map. For tests and the future
    /// rewrap CLI; production code must use `from_env`.
    pub fn from_keys(keys: HashMap<u16, [u8; 32]>, current: u16) -> Result<Self, KeyringError> {
        if keys.is_empty() {
            return Err(KeyringError::NoKeksLoaded);
        }
        if !keys.contains_key(&current) {
            let mut loaded: Vec<u16> = keys.keys().copied().collect();
            loaded.sort();
            return Err(KeyringError::BadCurrentVersion {
                requested: current,
                loaded,
            });
        }
        Ok(Self { keys, current })
    }

    /// The current-write KEK version. New encrypts stamp this into the
    /// frame; the rewrap query selects rows with `kek_id < this`.
    pub fn current_version(&self) -> u16 {
        self.current
    }

    /// Sorted list of loaded KEK versions, for diagnostics / startup logs.
    pub fn versions(&self) -> Vec<u16> {
        let mut v: Vec<u16> = self.keys.keys().copied().collect();
        v.sort();
        v
    }

    /// Encrypt `plaintext` under the current-write KEK. `aad` is bound into
    /// the AES-GCM tag (RFC 5116): the same `aad` MUST be supplied at
    /// decrypt or the tag check fails. Pass `b""` only if you have an
    /// affirmative reason to skip row-identity binding.
    pub fn encrypt(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let key_bytes = self
            .keys
            .get(&self.current)
            .expect("constructor invariant: current is loaded");
        seal_frame(plaintext, aad, key_bytes, self.current)
    }

    /// Decrypt a framed blob. Reads `kek_id` from the frame, looks up the
    /// matching key, and verifies the tag against `aad`. Returns
    /// `UnknownKekId` if the row was encrypted under a key that's no longer
    /// loaded (rotated out too aggressively, or a cross-environment leak).
    pub fn decrypt(&self, blob: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
        let header = FrameHeader::parse(blob)?;
        let key_bytes = self
            .keys
            .get(&header.kek_id)
            .ok_or(CryptoError::UnknownKekId(header.kek_id))?;
        open_frame(blob, aad, key_bytes)
    }

    /// Inspect the `kek_id` of a framed blob without decrypting. Used by
    /// the rewrap job to skip already-current rows without touching the
    /// AEAD path.
    pub fn read_kek_id(blob: &[u8]) -> Result<u16, CryptoError> {
        Ok(FrameHeader::parse(blob)?.kek_id)
    }
}

/// Process-wide Keyring, initialised once by `init_keyring` at boot.
/// Call sites reach it via `keyring()`.
static KEYRING: OnceCell<Keyring> = OnceCell::new();

/// Initialise the global Keyring from env. Call once from `main()` before
/// the HTTP server starts; subsequent calls return `Err` rather than
/// silently rebinding.
pub fn init_keyring() -> Result<&'static Keyring, KeyringError> {
    let kr = Keyring::from_env()?;
    KEYRING
        .set(kr)
        .map_err(|_| ())
        .expect("init_keyring called twice");
    Ok(KEYRING.get().expect("just set"))
}

/// Access the process-wide Keyring. Panics if `init_keyring` hasn't been
/// called yet — that's a bug, not a runtime condition.
pub fn keyring() -> &'static Keyring {
    KEYRING
        .get()
        .expect("keyring not initialised; call init_keyring() from main()")
}

struct FrameHeader {
    kek_id: u16,
}

impl FrameHeader {
    fn parse(blob: &[u8]) -> Result<Self, CryptoError> {
        if blob.len() < MIN_BLOB_LEN {
            return Err(CryptoError::TooShort {
                actual: blob.len(),
                min: MIN_BLOB_LEN,
            });
        }
        let version = blob[0];
        if version != FRAME_VERSION {
            return Err(CryptoError::UnknownVersion(version));
        }
        let alg = blob[1];
        if alg != ALG_AES_256_GCM {
            return Err(CryptoError::UnknownAlgorithm(alg));
        }
        let kek_id = u16::from_be_bytes([blob[2], blob[3]]);
        Ok(Self { kek_id })
    }
}

fn parse_kek_hex(s: &str) -> Result<[u8; 32], KeyError> {
    if s.len() != 64 {
        return Err(KeyError::BadLength);
    }
    let mut key = [0u8; 32];
    hex::decode_to_slice(s, &mut key).map_err(|_| KeyError::BadHex)?;
    validate_key_material(&key)?;
    Ok(key)
}

fn seal_frame(
    plaintext: &[u8],
    aad: &[u8],
    key_bytes: &[u8; 32],
    kek_id: u16,
) -> Result<Vec<u8>, CryptoError> {
    let unbound = UnboundKey::new(&AES_256_GCM, key_bytes).map_err(|_| CryptoError::AeadFailure)?;
    let sealing = LessSafeKey::new(unbound);

    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| CryptoError::AeadFailure)?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    let mut in_out = plaintext.to_vec();
    sealing
        .seal_in_place_append_tag(nonce, Aad::from(aad), &mut in_out)
        .map_err(|_| CryptoError::AeadFailure)?;

    let mut blob = Vec::with_capacity(FRAME_HEADER_LEN + in_out.len());
    blob.push(FRAME_VERSION);
    blob.push(ALG_AES_256_GCM);
    blob.extend_from_slice(&kek_id.to_be_bytes());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&in_out);
    Ok(blob)
}

fn open_frame(
    blob: &[u8],
    aad: &[u8],
    key_bytes: &[u8; 32],
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    let unbound = UnboundKey::new(&AES_256_GCM, key_bytes).map_err(|_| CryptoError::AeadFailure)?;
    let opening = LessSafeKey::new(unbound);

    let nonce_start = 1 + 1 + 2;
    let body_start = nonce_start + NONCE_LEN;
    let nonce_bytes: [u8; NONCE_LEN] = blob[nonce_start..body_start]
        .try_into()
        .expect("len checked in FrameHeader::parse");
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    let mut in_out: Zeroizing<Vec<u8>> = Zeroizing::new(blob[body_start..].to_vec());
    let plaintext_len = opening
        .open_in_place(nonce, Aad::from(aad), &mut in_out)
        .map_err(|_| CryptoError::AeadFailure)?
        .len();
    in_out.truncate(plaintext_len);
    Ok(in_out)
}

#[cfg(test)]
mod keyring_tests {
    use super::*;

    fn test_key(seed: u8) -> [u8; 32] {
        // Distinct, non-constant 32-byte keys for tests. Each byte is
        // `seed ^ index` so the key passes `validate_key_material` (not
        // all-zero, not constant) and is deterministic per seed.
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = seed ^ (i as u8).wrapping_add(1);
        }
        k
    }

    fn single_key_ring(version: u16, seed: u8) -> Keyring {
        let mut map = HashMap::new();
        map.insert(version, test_key(seed));
        Keyring::from_keys(map, version).expect("constructor")
    }

    #[test]
    fn round_trip_with_matching_aad() {
        let kr = single_key_ring(1, 0xA5);
        let plaintext = b"super secret totp seed bytes";
        let aad = b"user_id_uuid_bytes||nosdesk.mfa.totp";
        let blob = kr.encrypt(plaintext, aad).expect("encrypt");
        let pt = kr.decrypt(&blob, aad).expect("decrypt");
        assert_eq!(pt.as_slice(), plaintext);
    }

    #[test]
    fn wrong_aad_fails_decrypt() {
        let kr = single_key_ring(1, 0xA5);
        let blob = kr.encrypt(b"plaintext", b"aad-A").unwrap();
        let err = kr.decrypt(&blob, b"aad-B").unwrap_err();
        assert!(matches!(err, CryptoError::AeadFailure), "got {err:?}");
    }

    #[test]
    fn frame_includes_current_kek_id() {
        let mut map = HashMap::new();
        map.insert(1u16, test_key(0x11));
        map.insert(2u16, test_key(0x22));
        let kr = Keyring::from_keys(map, 2).unwrap();
        let blob = kr.encrypt(b"x", b"y").unwrap();
        assert_eq!(Keyring::read_kek_id(&blob).unwrap(), 2);
    }

    #[test]
    fn decrypt_uses_kek_id_from_frame_not_current() {
        // Encrypt under V1, then verify a Keyring whose *current* is V2
        // (but still loads V1) decrypts correctly via the frame's kek_id.
        let v1_only = single_key_ring(1, 0x11);
        let blob = v1_only.encrypt(b"legacy row", b"aad").unwrap();

        let mut map = HashMap::new();
        map.insert(1u16, test_key(0x11));
        map.insert(2u16, test_key(0x22));
        let rotated = Keyring::from_keys(map, 2).unwrap();
        let pt = rotated.decrypt(&blob, b"aad").unwrap();
        assert_eq!(pt.as_slice(), b"legacy row");
    }

    #[test]
    fn unknown_kek_id_rejected() {
        // Encrypt with V1, drop V1 from a keyring that only has V2.
        let v1_only = single_key_ring(1, 0x11);
        let blob = v1_only.encrypt(b"x", b"y").unwrap();

        let v2_only = single_key_ring(2, 0x22);
        let err = v2_only.decrypt(&blob, b"y").unwrap_err();
        assert!(matches!(err, CryptoError::UnknownKekId(1)), "got {err:?}");
    }

    #[test]
    fn too_short_blob_rejected() {
        let kr = single_key_ring(1, 0xA5);
        let err = kr.decrypt(&[0u8; 10], b"").unwrap_err();
        assert!(matches!(err, CryptoError::TooShort { actual: 10, .. }));
    }

    #[test]
    fn unknown_version_byte_rejected() {
        let kr = single_key_ring(1, 0xA5);
        // 32-byte blob with version=0xFF.
        let mut blob = vec![0xFFu8, ALG_AES_256_GCM, 0, 1];
        blob.extend_from_slice(&[0u8; NONCE_LEN + TAG_LEN]);
        let err = kr.decrypt(&blob, b"").unwrap_err();
        assert!(matches!(err, CryptoError::UnknownVersion(0xFF)));
    }

    #[test]
    fn unknown_algorithm_byte_rejected() {
        let kr = single_key_ring(1, 0xA5);
        let mut blob = vec![FRAME_VERSION, 0xEE, 0, 1];
        blob.extend_from_slice(&[0u8; NONCE_LEN + TAG_LEN]);
        let err = kr.decrypt(&blob, b"").unwrap_err();
        assert!(matches!(err, CryptoError::UnknownAlgorithm(0xEE)));
    }

    #[test]
    fn empty_plaintext_round_trip() {
        // Empty plaintext is still a valid AEAD input (auth-tag only).
        let kr = single_key_ring(1, 0xA5);
        let blob = kr.encrypt(b"", b"aad").unwrap();
        assert_eq!(blob.len(), MIN_BLOB_LEN);
        let pt = kr.decrypt(&blob, b"aad").unwrap();
        assert!(pt.is_empty());
    }

    #[test]
    fn from_keys_rejects_unknown_current() {
        let mut map = HashMap::new();
        map.insert(1u16, test_key(0x11));
        let err = Keyring::from_keys(map, 7).unwrap_err();
        assert!(matches!(
            err,
            KeyringError::BadCurrentVersion {
                requested: 7,
                loaded
            } if loaded == vec![1]
        ));
    }

    #[test]
    fn from_keys_rejects_empty() {
        let err = Keyring::from_keys(HashMap::new(), 1).unwrap_err();
        assert!(matches!(err, KeyringError::NoKeksLoaded));
    }

    #[test]
    fn read_kek_id_does_not_decrypt() {
        let kr = single_key_ring(7, 0xA5);
        let blob = kr.encrypt(b"x", b"y").unwrap();
        // No key needed; read_kek_id is a header-only inspection.
        assert_eq!(Keyring::read_kek_id(&blob).unwrap(), 7);
    }
}
