//! Lint: every email the server queues carries the workspace's security note.
//!
//! The note is added by whoever builds the message: a letter renders it
//! through the letterhead (`EmailTemplate`, via a `compose_` builder), and the
//! other builders append the shared `security_note` text. The merge notice and
//! the notification digest were built by hand and went out without it. This
//! holds every `NewOutboundEmail` construction to one of those, so a new
//! builder can't forget.
//!
//! ## Escape hatch
//!
//! A builder whose mail rightly goes without the note goes in `ALLOWLIST` as
//! (file, function) with a one-line reason.

#![allow(clippy::expect_used)]

use regex::Regex;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

/// Builders that queue mail without adding the note themselves.
const ALLOWLIST: &[(&str, &str)] = &[
    // Its callers close the reply with `with_security_note` before building
    // the row.
    ("services/channels/outbound.rs", "reply_row"),
    // An operator alert to NOSDESK_OPS_EMAIL, not mail from a workspace.
    (
        "services/transactional_email.rs",
        "prepare_bug_report_alert",
    ),
];

/// What shows a builder adds the note: it renders a letter, or appends the
/// shared text.
const NOTE_MARKERS: &[&str] = &["compose_", "EmailTemplate", "security_note"];

#[test]
fn every_queued_email_carries_the_security_note() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut missing = Vec::new();
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
        if relpath == "schema.rs" || relpath == "test_helpers.rs" || relpath.starts_with("models/")
        {
            continue;
        }
        let src = fs::read_to_string(entry.path()).expect("read source");
        missing.extend(builders_without_the_note(&relpath, &src));
    }
    assert!(
        missing.is_empty(),
        "queued email built without the security note; render it through a compose_ \
         letter, append `email_branding::security_note`, or add an ALLOWLIST entry with \
         a reason:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn a_builder_without_the_note_is_caught() {
    let bare = "fn prepare_x() -> NewOutboundEmail {\n    NewOutboundEmail {\n        body_text: t,\n    }\n}\n";
    assert_eq!(builders_without_the_note("services/x.rs", bare).len(), 1);
    let noted = "fn prepare_y() -> NewOutboundEmail {\n    let note = security_note(conn);\n    crate::models::NewOutboundEmail {\n        body_text: t,\n    }\n}\n";
    assert!(builders_without_the_note("services/x.rs", noted).is_empty());
    let letter = "    pub fn prepare_z(svc: &EmailService) -> NewOutboundEmail {\n        let (s, h, t) = svc.compose_invitation();\n        NewOutboundEmail { subject: s }\n    }\n";
    assert!(builders_without_the_note("services/x.rs", letter).is_empty());
}

/// `relpath:function` for each `NewOutboundEmail` built in `src` by a
/// function that neither adds the note nor is allowed not to.
fn builders_without_the_note(relpath: &str, src: &str) -> Vec<String> {
    let code = code_only(src);
    let construction = Regex::new(r"(->\s*)?(\w+::)*NewOutboundEmail\s*\{").unwrap();
    let fn_start = Regex::new(r"(?m)^([ \t]*)(pub(\([\w:]+\))?\s+)?(async\s+)?fn\s+(\w+)").unwrap();
    let mut missing = Vec::new();
    for m in construction.captures_iter(&code) {
        // A return type (`-> NewOutboundEmail {`) opens a body, not a row.
        if m.get(1).is_some() {
            continue;
        }
        let at = m.get(0).expect("match").start();
        let Some(f) = fn_start.captures_iter(&code[..at]).last() else {
            missing.push(format!("{relpath}: outside a function"));
            continue;
        };
        let name = f.get(5).expect("name").as_str();
        let start = f.get(0).expect("fn").start();
        let close = format!("\n{}}}", f.get(1).expect("indent").as_str());
        let end = code[at..].find(&close).map_or(code.len(), |e| at + e);
        let body = &code[start..end];
        if NOTE_MARKERS.iter().any(|marker| body.contains(marker)) {
            continue;
        }
        if ALLOWLIST.contains(&(relpath, name)) {
            continue;
        }
        missing.push(format!("{relpath}: {name}"));
    }
    missing
}

/// The source without `#[cfg(test)]` modules and `//` comment lines, so
/// fixtures and prose don't count.
fn code_only(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        if trimmed.starts_with("//") {
            i += 1;
            continue;
        }
        if trimmed.starts_with("#[cfg(test)]") {
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim().is_empty() {
                j += 1;
            }
            let next = lines.get(j).map(|l| l.trim_start()).unwrap_or("");
            if next.starts_with("mod ") || next.starts_with("pub mod ") {
                let mut depth = 0i32;
                let mut started = false;
                let mut k = j;
                while k < lines.len() {
                    for b in lines[k].bytes() {
                        match b {
                            b'{' => {
                                depth += 1;
                                started = true;
                            }
                            b'}' => depth -= 1,
                            _ => {}
                        }
                    }
                    k += 1;
                    if started && depth == 0 {
                        break;
                    }
                }
                i = k;
                continue;
            }
        }
        out.push(lines[i]);
        i += 1;
    }
    out.join("\n")
}
