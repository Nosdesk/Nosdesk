//! Lint: the process reaches libpq only through `db`.
//!
//! libpq initializes OpenSSL on its first connection, TLS or not, and that
//! first initialization registers OpenSSL's exit-time cleanup unless it
//! carries `OPENSSL_INIT_NO_ATEXIT`. The cleanup races the r2d2 worker
//! threads nothing joins, and crashes the process after its work is done
//! (`utils::openssl_exit`). `db::establish` and `db::ResettingManager::new`
//! set the flag before they connect, so every connection has to come from
//! one of them. This finds, anywhere under `src/` and `tests/`, test code
//! included:
//!
//! - `X::establish` for any `X` but `db` (`PgConnection::establish`,
//!   `<PgConnection as Connection>::establish`, a generic `C::establish`);
//! - `ConnectionManager::new`, with or without a turbofish;
//! - `pq_sys`, libpq's bindings.
//!
//! `src/db.rs` holds the two guarded doors, and `src/utils/openssl_exit.rs`
//! looks libpq up to find its libcrypto; both are exempt.

#![allow(clippy::expect_used)]

use regex::Regex;

use crate::lint_source::{mask, rust_sources_in};

const EXEMPT: &[&str] = &["src/db.rs", "src/utils/openssl_exit.rs"];

#[test]
fn libpq_is_reached_only_through_db() {
    let patterns = [
        Regex::new(r"\b(\w+)\s*>?\s*::\s*establish\b").expect("establish pattern"),
        Regex::new(r"\bConnectionManager\s*(?:::\s*<[^>]*>\s*)?::\s*new\b")
            .expect("manager pattern"),
        Regex::new(r"\bpq_sys\b").expect("pq_sys pattern"),
    ];

    let mut violations = Vec::new();
    for dir in ["src", "tests"] {
        for (rel, src) in rust_sources_in(dir) {
            let path = format!("{dir}/{rel}");
            if EXEMPT.contains(&path.as_str()) {
                continue;
            }
            let code = mask(&src, true);
            for re in &patterns {
                for caps in re.captures_iter(&code) {
                    if caps.get(1).is_some_and(|m| m.as_str() == "db") {
                        continue;
                    }
                    let m = caps.get(0).expect("whole match");
                    let line = src[..m.start()].matches('\n').count() + 1;
                    violations.push(format!("  {path}:{line}: {}", m.as_str()));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "\nThese connect to Postgres without going through `db`. libpq's first\n\
         connection initializes OpenSSL, and unless `db` has set\n\
         OPENSSL_INIT_NO_ATEXIT first, OpenSSL's exit-time cleanup races the\n\
         pools' worker threads and crashes the process as it exits\n\
         (utils::openssl_exit). Use `db::establish(url)` for one connection and\n\
         `db::ResettingManager::new(url)` for a pool.\n\n{}\n",
        violations.join("\n")
    );
}
