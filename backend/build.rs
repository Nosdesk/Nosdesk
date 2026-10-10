//! Build script.
//!
//! Computes a stable hash of the embedded migrations directory and
//! exposes it as the `NOSDESK_SCHEMA_HASH` env var so `db.rs` can
//! stamp it into `system_meta.schema_hash` on boot. The bootstrap
//! protocol uses this to detect client/server schema mismatches —
//! all we need is a value that changes deterministically when any
//! migration is added or modified, not a cryptographic primitive.
//!
//! Uses `std::collections::hash_map::DefaultHasher`, whose algorithm Rust
//! doesn't promise to keep, so nothing that outlives a build (a backup) may
//! depend on it.
//!
//! It also writes `migration_schema.rs` into `OUT_DIR`: for every prefix of
//! the migrations, its last migration's version and a SHA-256 of the files up
//! to it. Backups record the full set's entry, and restore reads a backup's
//! schema point back from this list.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

fn main() {
    // Read at run time, not with env!(): the compiled build script is reused
    // across checkouts that share a target dir (same unit hash for every
    // worktree), and a baked-in path would keep pointing at whichever
    // checkout compiled it. A stale path hashes a missing directory and
    // leaves the fingerprint permanently dirty, so every build recompiled.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let migrations_dir = Path::new(&manifest_dir).join("migrations");
    println!("cargo:rerun-if-changed={}", migrations_dir.display());

    let hash = hash_migrations_dir(&migrations_dir);
    println!("cargo:rustc-env=NOSDESK_SCHEMA_HASH={hash:016x}");

    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    fs::write(
        Path::new(&out_dir).join("migration_schema.rs"),
        migration_prefixes_source(&migrations_dir),
    )
    .expect("write migration_schema.rs");

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

/// `MIGRATION_PREFIXES`, as Rust source: one entry per migration directory, in
/// order, with the SHA-256 of every `.sql` file up to and including it. Each
/// file goes in as its path and its bytes, both length-prefixed, so no two
/// sets of files share a digest.
fn migration_prefixes_source(root: &Path) -> String {
    use sha2::{Digest, Sha256};

    let mut entries = walk_sql_files(root);
    entries.sort();
    let mut hasher = Sha256::new();
    let mut out = String::from("&[\n");
    let mut i = 0;
    while i < entries.len() {
        let dir = top_dir(&entries[i].0).to_string();
        while i < entries.len() && top_dir(&entries[i].0) == dir {
            let (relpath, content) = &entries[i];
            hasher.update((relpath.len() as u64).to_le_bytes());
            hasher.update(relpath.as_bytes());
            hasher.update((content.len() as u64).to_le_bytes());
            hasher.update(content);
            i += 1;
        }
        let version: String = dir
            .split('_')
            .next()
            .unwrap_or(&dir)
            .chars()
            .filter(|c| *c != '-')
            .collect();
        let digest: String = hasher
            .clone()
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        out.push_str(&format!(
            "    MigrationPrefix {{ version: \"{version}\", sha256: \"{digest}\" }},\n"
        ));
    }
    out.push(']');
    out
}

/// A migration file's directory: the first component of its relative path.
fn top_dir(relpath: &str) -> &str {
    relpath.split('/').next().unwrap_or(relpath)
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
