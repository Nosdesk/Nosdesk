//! CI guardrail against PII/secret interpolation into logs. Scans the crate
//! source for the two accidental-leak shapes and fails the build if either
//! appears, so leaks can't reach production between reviews.
//!
//! This is the *source-side* backstop; the *runtime* backstop is
//! `utils::redact::scrub` (masks emails/JWTs in emitted messages) and the
//! `tracing_redact` field-name allowlist. Belt, suspenders, and a lint.
//!
//! Why it matters here specifically: the redaction allowlist protects field
//! *names*, but `"message"` is always allowlisted and emitted verbatim — so a
//! customer email/subject/body splatted into a `info!("… {}", x)` message
//! string ships raw (this is exactly how `password_reset` leaked an email at
//! INFO). This scan is the thing that catches that class at CI time.
//!
//! It intentionally does NOT need a database — a pure static scan, so it runs
//! on every `cargo test` regardless of DB availability.

use std::fs;
use std::path::{Path, PathBuf};

/// Identifiers that must never be format-interpolated raw into a log message.
/// Values behind these names are PII, secrets, or customer content; log a
/// masked form (`mask_email`), a stable ID, or nothing instead.
///
/// Kept high-confidence on purpose: bare, never-legitimately-raw names. Generic
/// words (`name`, `content`) are omitted — they log config identifiers far more
/// often than customer data, and false positives would erode the guardrail.
const FORBIDDEN_IDENTS: &[&str] = &[
    // credentials / secrets
    "password",
    "new_password",
    "password_cleartext",
    "plaintext",
    "secret",
    "client_secret",
    "api_key",
    "access_token",
    "refresh_token",
    "id_token",
    "session_token",
    "csrf_token",
    "totp_secret",
    "recovery_code",
    "backup_code",
    // PII
    "email",
    "user_email",
    "requester_email",
    // customer content (a helpdesk's tickets/messages are sensitive)
    "subject",
    "ticket_subject",
    "ticket_body",
    "message_body",
    "comment_body",
];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir src").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Blank out `//` line and `/* */` block comments (respecting string/char
/// literals) so documentation that *mentions* the anti-pattern — e.g. a module
/// doc showing `info!("user={}", email)` as the thing to avoid — doesn't trip
/// the scan. Comment bytes become spaces so byte offsets are preserved.
fn strip_comments(content: &str) -> String {
    let bytes = content.as_bytes();
    let mut out = content.to_string();
    let buf = unsafe { out.as_bytes_mut() };
    let (mut in_str, mut in_char, mut escape) = (false, false, false);
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str || in_char {
            if escape {
                escape = false;
            } else if c == b'\\' {
                escape = true;
            } else if (in_str && c == b'"') || (in_char && c == b'\'') {
                in_str = false;
                in_char = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'\'' => in_char = true,
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    buf[i] = b' ';
                    i += 1;
                }
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    if bytes[i] != b'\n' {
                        buf[i] = b' ';
                    }
                    i += 1;
                }
                if i + 1 < bytes.len() {
                    buf[i] = b' ';
                    buf[i + 1] = b' ';
                    i += 2;
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// Extract the argument text of every `info!/warn!/error!/debug!/trace!`
/// (optionally `log::`/`tracing::`-prefixed) call — the substring between the
/// macro's outer parens — via a balanced-paren scan that skips string/char
/// literals. Scoping the checks to these spans keeps struct literals
/// (`Foo { subject: … }`), URL/email-body `format!`s, and operator `println!`s
/// from tripping the guardrail: only genuine log calls are scanned.
fn log_macro_arg_spans(content: &str) -> Vec<String> {
    use regex::Regex;
    let head = Regex::new(r"(?:log::|tracing::)?(?:info|warn|error|debug|trace)!\s*\(").unwrap();
    let mut spans = Vec::new();
    for m in head.find_iter(content) {
        let rest = &content[m.end()..]; // just past the opening '('
        let mut depth = 1i32;
        let (mut in_str, mut in_char, mut escape) = (false, false, false);
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            if escape {
                escape = false;
                continue;
            }
            match (in_str, in_char, c) {
                (true, _, '\\') | (_, true, '\\') => escape = true,
                (true, _, '"') => in_str = false,
                (_, true, '\'') => in_char = false,
                (true, _, _) | (_, true, _) => {}
                (_, _, '"') => in_str = true,
                (_, _, '\'') => in_char = true,
                (_, _, '(') => depth += 1,
                (_, _, ')') => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + c.len_utf8(); // include the closing ')'
                        break;
                    }
                }
                _ => {}
            }
        }
        spans.push(rest[..end].to_string());
    }
    spans
}

/// The first string literal in a macro-arg span — the format string.
fn first_string_literal(span: &str) -> Option<&str> {
    let start = span.find('"')? + 1;
    let rest = &span[start..];
    let mut escape = false;
    for (i, c) in rest.char_indices() {
        if escape {
            escape = false;
        } else if c == '\\' {
            escape = true;
        } else if c == '"' {
            return Some(&rest[..i]);
        }
    }
    None
}

/// Two leak shapes, scoped to log-macro call sites:
///
/// 1. **Inline capture** — `{email}` / `{subject:?}` *inside the format string*.
///    A capture in the literal is a variable substitution; a struct literal
///    can't appear inside a string, so this can't collide with `Foo { subject }`.
/// 2. **Positional** — `warn!("… {} …", email)`: an empty `{}` placeholder plus
///    a forbidden *bare* trailing arg (`,\s*&?IDENT\s*[,)]`), so `mask_email(&email)`
///    and struct-field `subject:` are not flagged. Field-access forms like
///    `user.email` are intentionally NOT caught here (the runtime `scrub` masks
///    those); this scan targets the unambiguous bare-identifier footgun.
fn violations_in(content: &str) -> Vec<String> {
    use regex::Regex;
    let alt = FORBIDDEN_IDENTS.join("|");
    let inline = Regex::new(&format!(r"\{{\s*({alt})\s*(?::[^}}]*)?\}}")).unwrap();
    let positional = Regex::new(&format!(
        r#"(?s)"[^"]*\{{\}}[^"]*"[^;]*?,\s*&?({alt})\s*[,)]"#
    ))
    .unwrap();

    let mut hits = Vec::new();
    for span in log_macro_arg_spans(content) {
        if let Some(fmt) = first_string_literal(&span) {
            for caps in inline.captures_iter(fmt) {
                hits.push(format!("inline capture `{{{}}}`", &caps[1]));
            }
        }
        for caps in positional.captures_iter(&span) {
            hits.push(format!("positional log arg `{}`", &caps[1]));
        }
    }
    hits
}

#[test]
fn no_pii_or_secret_interpolation_in_logs() {
    let mut files = Vec::new();
    rs_files(&src_dir(), &mut files);
    assert!(!files.is_empty(), "found no source files to scan");

    let mut failures = Vec::new();
    for file in &files {
        let raw = fs::read_to_string(file).expect("read source");
        let content = strip_comments(&raw);
        for hit in violations_in(&content) {
            failures.push(format!("{}: {hit}", file.display()));
        }
    }

    assert!(
        failures.is_empty(),
        "PII/secret/customer-content interpolated into a log message — mask it \
         (mask_email), log a stable ID, or drop it:\n  {}",
        failures.join("\n  ")
    );
}

/// The guardrail must actually detect the shapes it claims to — otherwise a
/// regex typo would render it a silent no-op that always passes.
#[test]
fn guardrail_detects_known_bad_patterns() {
    assert!(!violations_in(r#"info!("user {email} signed in");"#).is_empty());
    assert!(!violations_in(r#"warn!("login for {}", email);"#).is_empty());
    assert!(!violations_in(r#"error!("reset {new_password:?}");"#).is_empty());
    assert!(!violations_in(r#"trace!("Added proxy email: {}", email);"#).is_empty());
    assert!(!violations_in(r#"info!("ticket {}", subject);"#).is_empty());
    assert!(!violations_in("warn!(\n  \"verify {}\",\n  access_token,\n);").is_empty());

    // …and must NOT flag the safe forms the codebase actually uses:
    assert!(violations_in(r#"info!("no active user for that email");"#).is_empty());
    assert!(violations_in(r#"info!(email = %login.email, "login");"#).is_empty());
    assert!(violations_in(r#"info!("reset: {}", mask_email(&email));"#).is_empty());
    assert!(violations_in(r#"warn!("rate limited: path={}", req.path());"#).is_empty());
    assert!(violations_in(r#"error!(error = ?e, "db pool failed");"#).is_empty());
}

/// Comment-stripping must neutralise doc examples but keep real code on the same
/// or following lines, and must not treat `//` inside a string as a comment.
#[test]
fn strip_comments_blanks_comments_but_keeps_code() {
    let doc = "//! avoid: info!(\"user={}\", email)\nlet x = 1;";
    assert!(violations_in(&strip_comments(doc)).is_empty());
    let mixed = "// note\nwarn!(\"login {}\", email);";
    assert!(!violations_in(&strip_comments(mixed)).is_empty());
    let url = r#"info!("fetching https://api.example.com/{}", email);"#;
    assert!(!violations_in(&strip_comments(url)).is_empty());
}
