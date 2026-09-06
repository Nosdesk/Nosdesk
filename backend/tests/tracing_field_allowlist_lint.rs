//! Every structured field on a `tracing` macro must be on the redaction
//! allowlist, or it is silently dropped at the output boundary.
//!
//! `RedactingJsonLayer` emits only fields named in `ALLOWED_FIELDS` and counts
//! the rest under `redacted`. That is the right default (a field nobody
//! reviewed should not reach a log shipper), but it fails quietly: the code
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
//! ## How it gates: a ratchet
//!
//! The allowlist is deny-by-default, so "this field is dropped" is the normal
//! case and most of what the scanner finds is the filter working correctly.
//! `email` and `raw` are *supposed* to be dropped. A lint that fails on all of
//! it would be noise, so instead today's drops are recorded in
//! `tracing_field_allowlist_baseline.txt` and the gate fires on the difference:
//!
//! * a drop that is **not** in the baseline fails the build, so new code is
//!   guarded from its first commit;
//! * a baseline entry that no longer occurs **also** fails the build, so the
//!   file shrinks as sites are fixed and cannot quietly rot into an amnesty.
//!
//! Regenerate it after a deliberate change:
//!
//! ```text
//! UPDATE_ALLOWLIST_BASELINE=1 cargo test --test tracing_field_allowlist_lint
//! ```
//!
//! Baseline entries are `path field` pairs, not line numbers: a field name in a
//! file is the unit a reviewer actually judges, and it does not churn every time
//! something above it moves.
//!
//! For the human-readable report with line numbers, which is what you want when
//! fixing sites rather than gating:
//!
//! ```text
//! cargo test --test tracing_field_allowlist_lint -- --ignored --nocapture
//! ```
//!
//! ## What is matched
//!
//! A `key = value` or shorthand `key` field on `info!` / `warn!` / `error!` /
//! `debug!` / `trace!`, including the multi-line call form, which is most of
//! the field-rich lines, and which the first version of this scanner could not
//! see. `%` and `?` sigils are stripped, since they name the same field. After
//! the message literal only `key = value` and sigil forms count, because a bare
//! identifier there is a format argument rather than a field.
//!
//! ## Escape hatch
//!
//! `// allowlist-exempt: <reason>` on or above the macro, for a field that is
//! deliberately dropped in production and only wanted in a local `pretty` run.

#![allow(clippy::expect_used)]

use std::collections::BTreeSet;
use std::path::Path;

use walkdir::WalkDir;

/// Where the recorded drops live, relative to the crate root.
const BASELINE: &str = "tests/tracing_field_allowlist_baseline.txt";

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

// ---------------------------------------------------------------------------
// Source scanning
//
// Rust source is walked once, aware of comments, string literals, raw strings
// and char literals, so a macro name inside a doc comment or a `"` inside a
// string cannot throw the parse off. Brace matching then gives the whole call
// regardless of how many lines it spans.
// ---------------------------------------------------------------------------

/// One `tracing` macro invocation found in source.
#[derive(Debug, PartialEq, Eq)]
struct Call {
    /// 1-based line of the macro name.
    line: usize,
    fields: Vec<String>,
}

const MACROS: [&str; 5] = ["info", "warn", "error", "debug", "trace"];

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Advance past a `"..."` literal starting at `i`, returning the index just
/// after the closing quote.
fn skip_string(b: &[u8], mut i: usize, line: &mut usize) -> usize {
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            b'\n' => {
                *line += 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    i
}

/// Match `r"..."` / `r#"..."#` / `br##"..."##` at `i`, returning the index just
/// after the terminator, or `None` if this is not a raw-string opener.
fn skip_raw_string(b: &[u8], i: usize, line: &mut usize) -> Option<usize> {
    let mut j = i;
    if b.get(j) == Some(&b'b') {
        j += 1;
    }
    if b.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let hashes = {
        let start = j;
        while b.get(j) == Some(&b'#') {
            j += 1;
        }
        j - start
    };
    if b.get(j) != Some(&b'"') {
        return None;
    }
    j += 1;
    // The terminator is `"` followed by the same number of `#`.
    while j < b.len() {
        if b[j] == b'\n' {
            *line += 1;
        }
        if b[j] == b'"' && b[j + 1..].iter().take(hashes).all(|c| *c == b'#') {
            return Some(j + 1 + hashes);
        }
        j += 1;
    }
    Some(j)
}

/// Advance past a char literal such as `'a'`, `'\n'` or `'"'`. Returns `None`
/// for a lifetime (`'a` in `&'a str`), which must not be treated as a literal.
fn skip_char_literal(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i + 1) == Some(&b'\\') {
        let mut j = i + 2;
        while j < b.len() && b[j] != b'\'' {
            j += 1;
        }
        return Some(j + 1);
    }
    // A single character followed by a closing quote. Anything else at this
    // point is a lifetime.
    if b.get(i + 2) == Some(&b'\'') {
        return Some(i + 3);
    }
    None
}

/// Step over whatever non-code construct starts at `i`, or return `None` if `i`
/// is ordinary code. Shared by the top-level walk and the brace matcher so both
/// agree on what counts as a string.
fn skip_noncode(b: &[u8], i: usize, line: &mut usize) -> Option<usize> {
    match b[i] {
        b'\n' => {
            *line += 1;
            Some(i + 1)
        }
        b'/' if b.get(i + 1) == Some(&b'/') => {
            let mut j = i;
            while j < b.len() && b[j] != b'\n' {
                j += 1;
            }
            Some(j)
        }
        b'/' if b.get(i + 1) == Some(&b'*') => {
            let mut j = i + 2;
            let mut depth = 1usize;
            while j < b.len() && depth > 0 {
                if b[j] == b'\n' {
                    *line += 1;
                    j += 1;
                } else if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                    depth += 1;
                    j += 2;
                } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                    depth -= 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            Some(j)
        }
        b'r' | b'b' => skip_raw_string(b, i, line),
        b'"' => Some(skip_string(b, i, line)),
        b'\'' => skip_char_literal(b, i),
        _ => None,
    }
}

/// Given the index of an opening `(`, return the index of its match.
fn matching_paren(b: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    let mut scratch = 0usize;
    while i < b.len() {
        if let Some(next) = skip_noncode(b, i, &mut scratch) {
            i = next;
            continue;
        }
        match b[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// `info` / `warn` / ... immediately followed by `!` and `(`, at a word
/// boundary. Returns the index of the opening paren.
fn macro_open_at(b: &[u8], i: usize) -> Option<usize> {
    if i > 0 && is_ident_byte(b[i - 1]) {
        return None;
    }
    for name in MACROS {
        let n = name.len();
        if b[i..].starts_with(name.as_bytes()) && b.get(i + n) == Some(&b'!') {
            let mut j = i + n + 1;
            while b.get(j).is_some_and(|c| c.is_ascii_whitespace()) {
                j += 1;
            }
            if b.get(j) == Some(&b'(') {
                return Some(j);
            }
        }
    }
    None
}

/// Split an argument list on commas that are not nested inside brackets or a
/// string.
fn split_args(inner: &str) -> Vec<String> {
    let b = inner.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut i = 0usize;
    let mut scratch = 0usize;
    while i < b.len() {
        if let Some(next) = skip_noncode(b, i, &mut scratch) {
            i = next;
            continue;
        }
        match b[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                out.push(inner[start..i].to_owned());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(inner[start..].to_owned());
    out
}

/// Field names carried by a macro argument list.
///
/// Ordering matters: `tracing` puts fields before the message, and anything
/// after it that is not explicitly `key = value` or sigil-prefixed is a format
/// argument, not a field.
fn fields_in(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen_message = false;
    for arg in split_args(inner) {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }
        // `target: "..."`, `parent: span`, `name: "..."` are macro directives.
        if let Some((head, _)) = arg.split_once(':') {
            if matches!(head.trim(), "target" | "parent" | "name") {
                continue;
            }
        }
        if arg.starts_with('"') || arg.starts_with("r\"") || arg.starts_with("r#\"") {
            seen_message = true;
            continue;
        }
        let sigil = arg.starts_with('%') || arg.starts_with('?');
        let body = arg.trim_start_matches(['%', '?']).trim();
        // `=` but not `==` / `=>` / `!=`.
        let named = body.find('=').filter(|&e| {
            body.as_bytes().get(e + 1) != Some(&b'=')
                && body.as_bytes().get(e + 1) != Some(&b'>')
                && (e == 0 || body.as_bytes()[e - 1] != b'!')
        });
        let name = match named {
            Some(e) => body[..e].trim(),
            None => body,
        };
        // Past the message, a bare identifier is a format argument.
        if seen_message && named.is_none() && !sigil {
            continue;
        }
        if name.is_empty()
            || name.as_bytes()[0].is_ascii_digit()
            || !name.bytes().all(is_ident_byte)
        {
            continue;
        }
        out.push(name.to_owned());
    }
    out
}

/// Every tracing macro call in a source file, with its fields.
fn calls_in(src: &str) -> Vec<Call> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;
    while i < b.len() {
        if let Some(next) = skip_noncode(b, i, &mut line) {
            i = next;
            continue;
        }
        if let Some(open) = macro_open_at(b, i) {
            if let Some(close) = matching_paren(b, open) {
                out.push(Call {
                    line,
                    fields: fields_in(&src[open + 1..close]),
                });
            }
            // Carry on from just inside the parens rather than past the call,
            // so a nested macro is still seen and the line counter stays honest.
            i = open + 1;
            continue;
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// The scan
// ---------------------------------------------------------------------------

/// A dropped field: which file, which name, and the lines it occurs on.
struct Drop {
    key: String,
    lines: Vec<usize>,
}

/// Scan `src/` and return every field that the allowlist would drop.
fn dropped_fields() -> Vec<Drop> {
    let allowed = allowlist();
    assert!(
        allowed.contains("message"),
        "sanity: the allowlist parsed, and `message` is on it"
    );

    let mut by_key: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
    let mut files = 0usize;
    for entry in WalkDir::new("src").into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        // The redaction layer's own tests name fields deliberately.
        if path.ends_with("tracing_redact.rs") {
            continue;
        }
        files += 1;
        let src = std::fs::read_to_string(path).expect("read source");
        let lines: Vec<&str> = src.lines().collect();
        for call in calls_in(&src) {
            let above = call.line.checked_sub(2).and_then(|n| lines.get(n));
            let here = lines.get(call.line - 1);
            if [above, here]
                .into_iter()
                .flatten()
                .any(|l| l.contains("allowlist-exempt:"))
            {
                continue;
            }
            for field in call.fields {
                if allowed.contains(&field) {
                    continue;
                }
                by_key
                    .entry(format!("{} {}", normalise(path), field))
                    .or_default()
                    .push(call.line);
            }
        }
    }
    assert!(files > 100, "sanity: the walk found source to scan");
    by_key
        .into_iter()
        .map(|(key, lines)| Drop { key, lines })
        .collect()
}

/// Forward slashes, so the baseline is identical on every platform.
fn normalise(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn read_baseline() -> BTreeSet<String> {
    std::fs::read_to_string(BASELINE)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

// ---------------------------------------------------------------------------
// Gates
// ---------------------------------------------------------------------------

/// The ratchet. Fails on a drop that is not recorded, and equally on a recorded
/// drop that no longer happens. A baseline that only ever grows is an amnesty,
/// not a ratchet.
#[test]
fn no_new_fields_are_silently_dropped() {
    let current: BTreeSet<String> = dropped_fields().into_iter().map(|d| d.key).collect();

    if std::env::var("UPDATE_ALLOWLIST_BASELINE").is_ok() {
        let body = format!(
            "# Fields dropped by the redaction allowlist today, one `path field` per\n\
             # line. Generated by:\n\
             #\n\
             #   UPDATE_ALLOWLIST_BASELINE=1 cargo test --test tracing_field_allowlist_lint\n\
             #\n\
             # Most entries are the filter working correctly. The ones that are not are\n\
             # operator-facing lines shipping empty; deleting a line here after either\n\
             # allowlisting the field or marking the call `// allowlist-exempt:` is how\n\
             # this file shrinks. It is not allowed to rot: an entry that no longer\n\
             # occurs fails the build too.\n\
             {}\n",
            current.iter().cloned().collect::<Vec<_>>().join("\n")
        );
        std::fs::write(BASELINE, body).expect("write baseline");
        return;
    }

    let baseline = read_baseline();
    let added: Vec<&String> = current.difference(&baseline).collect();
    let fixed: Vec<&String> = baseline.difference(&current).collect();

    assert!(
        added.is_empty(),
        "These tracing fields are new since the baseline and would be dropped at the \
         output boundary, so the log line ships with its message and none of its data:\n  \
         {}\n\nAdd the field to ALLOWED_FIELDS in src/utils/tracing_redact.rs if it is \
         safe to emit (bounded values, no user content), or mark the call \
         `// allowlist-exempt: <reason>` if the drop is intended.",
        added
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );

    assert!(
        fixed.is_empty(),
        "These baseline entries no longer occur, so the ratchet has tightened. Remove \
         them from {BASELINE} so it cannot drift back:\n  {}\n\nOr regenerate with \
         `UPDATE_ALLOWLIST_BASELINE=1 cargo test --test tracing_field_allowlist_lint`.",
        fixed
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// The full report with line numbers, for working through sites rather than
/// gating. Ignored because it is output, not a pass/fail.
#[test]
#[ignore = "report, not a gate: run with --ignored --nocapture"]
fn report_dropped_fields() {
    let drops = dropped_fields();
    let sites: usize = drops.iter().map(|d| d.lines.len()).sum();
    println!(
        "{} dropped field(s) across {sites} call site(s):\n",
        drops.len()
    );
    for d in &drops {
        let lines: Vec<String> = d.lines.iter().map(usize::to_string).collect();
        println!("  {} (line {})", d.key, lines.join(", "));
    }
}

// ---------------------------------------------------------------------------
// The scanner's own tests. A lint that finds nothing passes for the wrong
// reason, so the parser is tested against the forms it has to handle.
// ---------------------------------------------------------------------------

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

/// The gap that made the first version of this scanner report a fraction of the
/// truth: 440 of the codebase's 2056 tracing calls put their fields on their own
/// lines, and those are the field-rich ones that matter most.
#[test]
fn a_multiline_call_is_read_whole() {
    let src = r#"
fn boot() {
    info!(
        host = %cfg.host,
        port = cfg.port,
        "Listening"
    );
}
"#;
    let calls = calls_in(src);
    assert_eq!(calls.len(), 1, "one call: {calls:?}");
    assert_eq!(calls[0].fields, vec!["host", "port"], "{calls:?}");
    assert_eq!(calls[0].line, 3, "reports the macro's own line: {calls:?}");
}

/// A macro name in a comment or a string is not a call, and a `"` inside a
/// string must not desynchronise the walk.
#[test]
fn comments_and_strings_are_not_code() {
    let src = r#"
// info!(fake_field = 1, "not real")
fn f() {
    let s = "a ) paren and an info!(also_fake) inside a string";
    warn!(real_field = 1, "real");
}
"#;
    let fields: Vec<String> = calls_in(src).into_iter().flat_map(|c| c.fields).collect();
    assert_eq!(fields, vec!["real_field"], "{fields:?}");
}

/// After the message, a bare identifier is a format argument. Reporting it as a
/// dropped field would be a false positive, and a lint with those gets ignored.
#[test]
fn format_arguments_are_not_fields() {
    let f = fields_in(r#""queued {} of {}", done, total"#);
    assert!(f.is_empty(), "format args are not fields: {f:?}");
    let g = fields_in(r#""done", elapsed_ms = t, %kind"#);
    assert_eq!(
        g,
        vec!["elapsed_ms", "kind"],
        "explicit fields still count: {g:?}"
    );
}

/// A value containing a comma must not be split into a second field.
#[test]
fn a_nested_comma_does_not_split_a_field() {
    let f = fields_in(r#"count = items.get(a, b), "msg""#);
    assert_eq!(f, vec!["count"], "{f:?}");
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
