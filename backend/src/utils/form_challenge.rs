//! A small proof-of-work challenge for the public request form (the scheme
//! ALTCHA uses: HMAC-signed, SHA-256, solved in the browser). It costs a real
//! visitor a fraction of a second, invisibly and without a puzzle, and makes
//! every automated submission pay the same.
//!
//! The server picks a salt and a secret number, and sends
//! `challenge = SHA-256(salt + number)` with an HMAC signature of it. The
//! browser tries numbers until one hashes to `challenge`. The salt carries the
//! workspace and the issue and expiry times, and it is inside the hash, so none
//! of them can be changed without breaking the signature. A form sent sooner
//! than [`MIN_SECONDS`] after the challenge was issued is refused too (people
//! take longer than that to type a request), and each solution works once (the
//! caller records [`Verified::replay_key`]).

use ring::{digest, hmac};
use serde::{Deserialize, Serialize};

const KEY_LABEL: &[u8] = b"nosdesk-form-challenge-v1";
/// Upper bound of the secret number: about 150,000 hashes on average, a
/// quarter of a second on a desktop browser and a couple of seconds on a slow
/// phone, spent while the person is still typing.
pub const MAX_NUMBER: u64 = 300_000;
/// Soonest a form may be sent after its challenge was issued.
pub const MIN_SECONDS: i64 = 3;
/// How long a challenge stays good (a long form, a coffee break).
pub const TTL_SECONDS: i64 = 2 * 60 * 60;

/// What the browser gets.
#[derive(Debug, Serialize)]
pub struct Challenge {
    pub algorithm: &'static str,
    pub challenge: String,
    pub salt: String,
    pub signature: String,
    pub maxnumber: u64,
}

/// What the browser sends back.
#[derive(Debug, Clone, Deserialize)]
pub struct Solution {
    pub challenge: String,
    pub salt: String,
    pub signature: String,
    pub number: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Verified {
    /// Record this until [`Verified::expires`] so the solution isn't reused.
    pub replay_key: String,
    pub expires: i64,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(input: &str) -> String {
    hex(digest::digest(&digest::SHA256, input.as_bytes()).as_ref())
}

fn key() -> Option<hmac::Key> {
    let secret = std::env::var("JWT_SECRET").ok().filter(|s| !s.is_empty())?;
    let k = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    Some(hmac::Key::new(
        hmac::HMAC_SHA256,
        hmac::sign(&k, KEY_LABEL).as_ref(),
    ))
}

/// A new challenge for `workspace_id`, issued at `now` (unix seconds).
pub fn issue_with(
    key: &hmac::Key,
    workspace_id: i32,
    now: i64,
    number: u64,
    nonce: &str,
) -> Challenge {
    let salt = format!(
        "{nonce}?ws={workspace_id}&iat={now}&exp={}&",
        now + TTL_SECONDS
    );
    let challenge = sha256_hex(&format!("{salt}{number}"));
    let signature = hex(hmac::sign(key, challenge.as_bytes()).as_ref());
    Challenge {
        algorithm: "SHA-256",
        challenge,
        salt,
        signature,
        maxnumber: MAX_NUMBER,
    }
}

pub fn issue(workspace_id: i32) -> Option<Challenge> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let number = rng.gen_range(0..=MAX_NUMBER);
    let nonce: String = (0..12)
        .map(|_| format!("{:02x}", rng.gen::<u8>()))
        .collect();
    Some(issue_with(
        &key()?,
        workspace_id,
        chrono::Utc::now().timestamp(),
        number,
        &nonce,
    ))
}

fn param(salt: &str, name: &str) -> Option<i64> {
    let query = salt.split_once('?')?.1;
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))?
        .parse()
        .ok()
}

/// Check a solution for `workspace_id` at `now`.
pub fn verify_with(
    key: &hmac::Key,
    workspace_id: i32,
    solution: &Solution,
    now: i64,
) -> Result<Verified, &'static str> {
    if solution.number > MAX_NUMBER || solution.salt.len() > 200 {
        return Err("malformed");
    }
    let signature = (0..solution.signature.len())
        .step_by(2)
        .map(|i| {
            solution
                .signature
                .get(i..i + 2)
                .and_then(|b| u8::from_str_radix(b, 16).ok())
        })
        .collect::<Option<Vec<u8>>>()
        .ok_or("malformed")?;
    hmac::verify(key, solution.challenge.as_bytes(), &signature).map_err(|_| "not ours")?;
    if sha256_hex(&format!("{}{}", solution.salt, solution.number)) != solution.challenge {
        return Err("wrong answer");
    }
    if param(&solution.salt, "ws") != Some(i64::from(workspace_id)) {
        return Err("another workspace");
    }
    let issued = param(&solution.salt, "iat").ok_or("malformed")?;
    let expires = param(&solution.salt, "exp").ok_or("malformed")?;
    if now < issued + MIN_SECONDS {
        return Err("too fast");
    }
    if now >= expires {
        return Err("expired");
    }
    Ok(Verified {
        replay_key: format!("form_challenge_used:{}", sha256_hex(&solution.challenge)),
        expires,
    })
}

pub fn verify(workspace_id: i32, solution: &Solution) -> Result<Verified, &'static str> {
    verify_with(
        &key().ok_or("no key")?,
        workspace_id,
        solution,
        chrono::Utc::now().timestamp(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solve(c: &Challenge) -> Solution {
        let number = (0..=c.maxnumber)
            .find(|n| sha256_hex(&format!("{}{n}", c.salt)) == c.challenge)
            .expect("solvable");
        Solution {
            challenge: c.challenge.clone(),
            salt: c.salt.clone(),
            signature: c.signature.clone(),
            number,
        }
    }

    #[test]
    fn a_solved_challenge_verifies_once_its_time_has_come() {
        let key = hmac::Key::new(hmac::HMAC_SHA256, b"test");
        let c = issue_with(&key, 7, 1_000, 4_321, "abc");
        let s = solve(&c);
        assert_eq!(s.number, 4_321);
        assert_eq!(verify_with(&key, 7, &s, 1_001), Err("too fast"));
        let ok = verify_with(&key, 7, &s, 1_010).unwrap();
        assert!(ok.replay_key.starts_with("form_challenge_used:"));
        assert_eq!(
            verify_with(&key, 7, &s, 1_000 + TTL_SECONDS),
            Err("expired")
        );
        assert_eq!(verify_with(&key, 8, &s, 1_010), Err("another workspace"));
    }

    #[test]
    fn tampering_or_guessing_fails() {
        let key = hmac::Key::new(hmac::HMAC_SHA256, b"test");
        let c = issue_with(&key, 7, 1_000, 99, "abc");
        let good = solve(&c);
        let wrong = Solution {
            number: good.number + 1,
            ..good.clone()
        };
        assert_eq!(verify_with(&key, 7, &wrong, 1_010), Err("wrong answer"));
        // A later expiry in the salt no longer hashes to the signed challenge.
        let stretched = Solution {
            salt: good.salt.replace("exp=8200", "exp=99999999"),
            ..good.clone()
        };
        assert!(verify_with(&key, 7, &stretched, 9_000).is_err());
        // A challenge we didn't sign.
        let forged = Solution {
            signature: "00".repeat(32),
            ..good.clone()
        };
        assert_eq!(verify_with(&key, 7, &forged, 1_010), Err("not ours"));
        let other_key = hmac::Key::new(hmac::HMAC_SHA256, b"other");
        assert_eq!(verify_with(&other_key, 7, &good, 1_010), Err("not ours"));
    }
}
