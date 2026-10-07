//! Lint: attachment sync events are built in one place, and only the
//! attachment repository writes the `attachments` table.
//!
//! Who receives an attachment's events depends on the row: a file on a reply
//! goes to its ticket's audience, a draft to its uploader, a guest's draft to
//! no one. When creates, claims and deletes each picked their own audience,
//! guest drafts reached the whole workspace and fired webhooks. Every event
//! now comes from `repository::comments::attachment_event`, and this keeps a
//! new writer from choosing its own.
//!
//! ## Escape hatch
//!
//! A write to `attachments` outside `repository/comments.rs` goes in
//! `WRITE_ALLOWLIST` with a one-line reason.

#![allow(clippy::expect_used)]

use regex::Regex;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

const EMITTER_FILE: &str = "repository/comments.rs";
const EMITTER_FN: &str = "fn attachment_event(";

/// Files outside the attachment repository that may write `attachments`.
const WRITE_ALLOWLIST: &[&str] = &[
    // Test fixtures insert rows directly.
    "test_helpers.rs",
    // Account deletion clears `uploaded_by`, which no sync event carries.
    "repository/users.rs",
];

#[test]
fn attachment_events_come_from_one_builder() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let event_re = Regex::new(r"aggregate:\s*SyncAggregate::Attachment\b").unwrap();
    let write_re = Regex::new(
        r"(?i)(insert_into|update|delete)\s*\(\s*attachments::table|(insert\s+into|update|delete\s+from)\s+(public\.)?attachments\b",
    )
    .unwrap();

    let mut stray_events = Vec::new();
    let mut stray_writes = Vec::new();
    let mut builder_events = 0;

    for entry in WalkDir::new(&src_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
    {
        let relpath = entry
            .path()
            .strip_prefix(&src_root)
            .expect("under src")
            .to_string_lossy()
            .replace('\\', "/");
        if relpath == "schema.rs" {
            continue;
        }
        let src = fs::read_to_string(entry.path()).expect("read source");

        // The builder's body runs from its signature to the next item.
        let builder = (relpath == EMITTER_FILE)
            .then(|| src.find(EMITTER_FN))
            .flatten()
            .map(|start| {
                let rest = &src[start + EMITTER_FN.len()..];
                let end = rest.find("\nfn ").or_else(|| rest.find("\npub fn "));
                start..end.map_or(src.len(), |e| start + EMITTER_FN.len() + e)
            });
        for m in event_re.find_iter(&src) {
            if builder.as_ref().is_some_and(|b| b.contains(&m.start())) {
                builder_events += 1;
            } else {
                stray_events.push(format!("{relpath} (byte {})", m.start()));
            }
        }

        if relpath != EMITTER_FILE && !WRITE_ALLOWLIST.contains(&relpath.as_str()) {
            for m in write_re.find_iter(&src) {
                stray_writes.push(format!("{relpath}: {}", m.as_str()));
            }
        }
    }

    assert_eq!(
        builder_events, 1,
        "expected exactly one attachment event in {EMITTER_FILE}::attachment_event"
    );
    assert!(
        stray_events.is_empty(),
        "attachment sync events built outside attachment_event; build them there so the \
         audience follows the row:\n  {}",
        stray_events.join("\n  ")
    );
    assert!(
        stray_writes.is_empty(),
        "writes to `attachments` outside {EMITTER_FILE}; go through its functions, which \
         emit, or add the file to WRITE_ALLOWLIST with a reason:\n  {}",
        stray_writes.join("\n  ")
    );
}
