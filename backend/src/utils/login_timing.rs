//! Equal-work credential verification.
//!
//! The login family of handlers used to branch on user existence
//! before doing the expensive bcrypt work, which leaked existence
//! via timing (`existing@example.com` ~80ms vs `nope@example.com`
//! ~5ms). This module collapses that into one path: every call
//! runs `bcrypt::verify` against either the user's real hash or
//! a pre-computed dummy hash, so the work is the same regardless
//! of whether the user exists.
//!
//! The dummy hash is built at startup via `bcrypt::hash` so it
//! has the same cost factor and the same parse shape as a real
//! hash. A statically-pasted string would skip the lazy compute,
//! but it would also lock in whatever cost we typed in once and
//! drift away from `DEFAULT_COST` as the bcrypt crate's default
//! moves; deriving it at runtime keeps the two in lockstep.
//!
//! SSO-only users (rows in `users` with no `local` row in
//! `user_auth_identities`) take the dummy path. Otherwise their
//! presence would leak via the missing-hash branch.

use std::sync::LazyLock;

use bcrypt::{hash, verify, DEFAULT_COST};
use diesel::prelude::*;
use rand::RngCore;

use crate::db::DbConnection;
use crate::models::User;
use crate::repository;
use crate::schema::user_auth_identities;

/// Real-shaped bcrypt hash that fails to verify against any
/// attacker-supplied input. The plaintext is 32 random bytes
/// generated at process start, so even an attacker who reads
/// the binary can't recover it. Same `DEFAULT_COST` as a real
/// signup, so the verify cost matches.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    let mut secret = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut secret);
    hash(&secret[..], DEFAULT_COST)
        .expect("dummy bcrypt hash generation must succeed at startup")
});

/// Returns the authenticated `User` when the email exists, has a
/// local password hash, and the supplied password verifies. Runs
/// `bcrypt::verify` against a dummy hash on every other path so
/// the wall-clock cost is the same.
pub fn verify_credentials(
    conn: &mut DbConnection,
    email: &str,
    password: &str,
) -> Option<User> {
    let (hash_to_check, candidate_user) = match lookup_user_and_hash(conn, email) {
        Some((user, hash)) => (hash, Some(user)),
        None => ((*DUMMY_HASH).clone(), None),
    };
    let matched = verify(password, &hash_to_check).unwrap_or(false);
    if matched {
        candidate_user
    } else {
        None
    }
}

/// Looks up the user + local password hash atomically. Returns
/// `None` when (a) no user exists for the email, (b) the user
/// has no `local` auth identity (SSO-only), or (c) the local
/// row exists but has a null password_hash. All three are
/// indistinguishable from the caller's perspective so they get
/// the same timing profile via the dummy hash.
fn lookup_user_and_hash(conn: &mut DbConnection, email: &str) -> Option<(User, String)> {
    let user = repository::users::get_user_by_email(email, conn).ok()?;
    let hash: Option<String> = user_auth_identities::table
        .filter(user_auth_identities::user_uuid.eq(user.uuid))
        .filter(user_auth_identities::provider_type.eq("local"))
        .select(user_auth_identities::password_hash)
        .first::<Option<String>>(conn)
        .optional()
        .ok()
        .flatten()
        .flatten();
    Some((user, hash?))
}

/// Force the dummy-hash lazy init. Called at startup so the
/// first real login attempt doesn't pay the one-shot
/// generation cost (~80ms) and reveal that it was the first.
pub fn prewarm() {
    let _ = LazyLock::force(&DUMMY_HASH);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dummy_hash_is_a_well_formed_bcrypt_string() {
        let h = LazyLock::force(&DUMMY_HASH);
        assert!(
            h.starts_with("$2") && h.len() >= 59,
            "expected a bcrypt-format hash, got: {h}"
        );
    }

    #[test]
    fn dummy_hash_rejects_arbitrary_inputs() {
        let h = LazyLock::force(&DUMMY_HASH);
        // The dummy plaintext is 32 random bytes generated at
        // process start; no attacker-supplied string verifies.
        for candidate in ["", "password", "hunter2", "admin", "correct horse battery staple"] {
            assert!(
                !verify(candidate, h).unwrap_or(false),
                "dummy hash should reject {candidate:?}"
            );
        }
    }

    /// Statistical timing equivalence between the dummy-hash
    /// path and a real verify against a freshly-generated hash.
    /// Ignored by default because the variance is high in CI;
    /// run locally with `cargo test -- --ignored login_timing`
    /// after touching anything in this module.
    #[test]
    #[ignore = "noisy; run locally after edits to confirm timing equivalence"]
    fn missing_path_and_existing_path_take_similar_time() {
        use std::time::Instant;

        let real_hash = hash("the-real-password", DEFAULT_COST).unwrap();
        let mut real = Vec::with_capacity(50);
        let mut dummy = Vec::with_capacity(50);

        for _ in 0..50 {
            let t = Instant::now();
            let _ = verify("wrong-password", &real_hash);
            real.push(t.elapsed());

            let t = Instant::now();
            let _ = verify("wrong-password", &DUMMY_HASH);
            dummy.push(t.elapsed());
        }

        real.sort();
        dummy.sort();
        let real_median = real[real.len() / 2];
        let dummy_median = dummy[dummy.len() / 2];
        let delta = if real_median > dummy_median {
            real_median - dummy_median
        } else {
            dummy_median - real_median
        };
        assert!(
            delta.as_millis() < 20,
            "real median {real_median:?} vs dummy median {dummy_median:?} (delta {delta:?})"
        );
    }
}
