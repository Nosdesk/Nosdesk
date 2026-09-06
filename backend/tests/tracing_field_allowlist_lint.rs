//! Every structured field on a `tracing` macro must be on the redaction
//! allowlist, or it is silently dropped at the output boundary.
//!
//! `RedactingJsonLayer` emits only fields named in `ALLOWED_FIELDS` and counts
//! the rest under `redacted`. That is the right default -- a field nobody
//! reviewed should not reach a log shipper -- but it fails quietly: the code
//! compiles, the tests pass, and the log line ships with its message and none
//! of its data.
//!
//! This is not hypothetical. The push boot line added in #306 shipped emitting
//!
//! ```text
//! {"fields":{"message":"Push sender selected"},"redacted":4}
//! ```
//!
//! because `mode`, `sender`, `configured` and `process_id` were never
//! allowlisted. The line existed precisely so an operator could tell relay mode
//! from native on a running instance, and it named nothing. It was found by
//! reading deployed output, which is not a repeatable way to catch a class of
//! bug.
//!
//! ## What is matched
//!
//! A `key = value` or shorthand `key,` field inside `info!` / `warn!` /
//! `error!` / `debug!` / `trace!`. The `%foo` and `?foo` sigils are stripped
//! first, since they name the same field.
//!
//! ## Escape hatch
//!
//! `// allowlist-exempt: <reason>` on the line above the macro, for a field
//! that is deliberately dropped in production and only wanted in a local
//! `pretty` run.

#![allow(clippy::expect_used)]

use std::collections::BTreeSet;
use std::path::Path;

use walkdir::WalkDir;

/// Pull `ALLOWED_FIELDS` out of the redaction layer's source rather than
/// importing it: the constant is private, and a lint that reads the same text a
/// reviewer would cannot silently disagree with it.
fn allowlist() -> BTreeSet<String> {
    let src = std::fs::read_to_string("src/utils/tracing_redact.rs").expect("read tracing_redact");
    let start = src
        .find("const ALLOWED_FIELDS")
        .expect("ALLOWED_FIELDS exists");
    let body = &src[start..src[start..].find("];").expect("list ends") + start];
    body.lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix('"')
                .and_then(|r| r.split('"').next())
                .filter(|_| l.starts_with('"'))
                .map(str::to_owned)
        })
        .collect()
}

/// Field names appearing in a tracing macro call, with sigils stripped.
fn fields_in(call: &str) -> Vec<String> {
    let mut out = Vec::new();
    // Only the argument list before the message literal carries fields.
    let head = match call.find('"') {
        Some(i) => &call[..i],
        None => call,
    };
    for part in head.split(',') {
        let p = part.trim().trim_start_matches(['%', '?']);
        let name = p
            .split('=')
            .next()
            .unwrap_or("")
            .trim()
            .trim_start_matches(['%', '?']);
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        // A bare `foo` shorthand or `foo = expr`; skip Rust keywords and
        // anything that is obviously a value rather than a name.
        if matches!(name, "self" | "true" | "false" | "if" | "match") {
            continue;
        }
        out.push(name.to_owned());
    }
    out
}

/// Ignored deliberately, and not yet a gate. Run it with
/// `cargo test --test tracing_field_allowlist_lint -- --ignored` to list every
/// field currently dropped at the output boundary.
///
/// It cannot fail the build as written, because the allowlist is deny-by-default:
/// "not on the allowlist" is the normal case, and almost all of the ~700 hits are
/// the filter doing its job (`email`, `user_principal_name`, `filename` should
/// all be dropped). The scanner itself works -- the two tests below prove it --
/// but the signal needs narrowing before it can gate anything.
///
/// What it did surface: **15 sites in `startup.rs` and the notification
/// channels** whose fields are dropped although the line exists to inform an
/// operator (`url`, `host`, `port`, `versions`, `current`, `path`, `plugin`).
/// Those boot lines ship as empty as the push boot line did before #312. Each
/// needs a per-field safety judgement -- `path` and `url` can carry credentials
/// -- which is why this is handed off rather than rushed.
#[test]
#[ignore = "reports dropped fields; see the doc comment before making it a gate"]
fn tracing_fields_are_allowlisted() {
    let allowed = allowlist();
    assert!(
        allowed.contains("message"),
        "sanity: the allowlist parsed, and `message` is on it"
    );

    let mut offences: Vec<String> = Vec::new();
    for entry in WalkDir::new("src").into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        // The redaction layer's own tests name fields deliberately.
        if path.ends_with("tracing_redact.rs") {
            continue;
        }
        let src = std::fs::read_to_string(path).expect("read source");
        let lines: Vec<&str> = src.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim();
            let Some(rest) = ["info!(", "warn!(", "error!(", "debug!(", "trace!("]
                .iter()
                .find_map(|m| t.split_once(m).map(|(_, r)| r))
            else {
                continue;
            };
            if i > 0 && lines[i - 1].contains("allowlist-exempt:") {
                continue;
            }
            for field in fields_in(rest) {
                if !allowed.contains(&field) {
                    offences.push(format!(
                        "{}:{} field `{}` is not in ALLOWED_FIELDS",
                        path.display(),
                        i + 1,
                        field
                    ));
                }
            }
        }
    }

    assert!(
        offences.is_empty(),
        "These tracing fields would be dropped at the output boundary, so the log line \
         ships with its message and none of its data:\n  {}\n\nAdd the field to \
         ALLOWED_FIELDS in src/utils/tracing_redact.rs if it is safe to emit (bounded \
         values, no user content), or mark the call `// allowlist-exempt: <reason>`.",
        offences.join("\n  ")
    );
}

/// The parser has to actually find fields, or the lint passes by finding
/// nothing and proves only that it ran.
#[test]
fn the_parser_recognises_fields() {
    let f = fields_in(r#"recipient = %uuid, count, "Push dispatched""#);
    assert!(f.contains(&"recipient".to_string()), "named field: {f:?}");
    assert!(f.contains(&"count".to_string()), "shorthand field: {f:?}");
    assert!(
        !f.iter().any(|x| x == "Push dispatched"),
        "the message literal is not a field: {f:?}"
    );
}

/// A lint that never fires is indistinguishable from one that cannot.
#[test]
fn an_unallowlisted_field_is_detected() {
    let allowed = allowlist();
    assert!(
        !allowed.contains("definitely_not_a_real_field_name"),
        "control: an invented name must not be on the allowlist"
    );
    let parsed = fields_in("definitely_not_a_real_field_name = 1, \"msg\"");
    assert_eq!(parsed, vec!["definitely_not_a_real_field_name".to_string()]);
}
