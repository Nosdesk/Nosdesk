//! Build script.
//!
//! Computes a stable hash of the embedded migrations directory and
//! exposes it as the `NOSDESK_SCHEMA_HASH` env var so `db.rs` can
//! stamp it into `system_meta.schema_hash` on boot. The bootstrap
//! protocol uses this to detect client/server schema mismatches —
//! all we need is a value that changes deterministically when any
//! migration is added or modified, not a cryptographic primitive.
//!
//! Uses `std::collections::hash_map::DefaultHasher` (SipHash-2-4) so
//! we don't pull in `sha2` for a non-security-relevant fingerprint.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

fn main() {
    let migrations_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    println!("cargo:rerun-if-changed={}", migrations_dir.display());

    let hash = hash_migrations_dir(&migrations_dir);
    println!("cargo:rustc-env=NOSDESK_SCHEMA_HASH={hash:016x}");

    // get_current_version() reads option_env!("NOSDESK_VERSION"); without this
    // a changed version wouldn't trigger a recompile of the crate that bakes it.
    println!("cargo:rerun-if-env-changed=NOSDESK_VERSION");
}

fn hash_migrations_dir(root: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    let mut entries: Vec<_> = walk_sql_files(root);
    // Sort so the hash is stable across filesystems with different
    // directory iteration orders.
    entries.sort();
    for (relpath, content) in entries {
        relpath.hash(&mut hasher);
        content.hash(&mut hasher);
    }
    hasher.finish()
}

fn walk_sql_files(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read_dir = match fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("sql") {
                continue;
            }
            // Re-emit cargo:rerun-if-changed for every individual
            // migration file too, so cargo's incremental rebuild fires
            // when any single file inside a subdir changes.
            println!("cargo:rerun-if-changed={}", path.display());
            let relpath = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if let Ok(bytes) = fs::read(&path) {
                out.push((relpath, bytes));
            }
        }
    }
    out
}
