//! DSN corpus regression test.
//!
//! Drives `services::channels::bounce_parser::parse_bounce` against a
//! set of real-world-shaped DSN samples checked in under
//! `tests/fixtures/dsn/`. Each sample exercises one realistic shape
//! the parser has to survive:
//!
//! - `postfix-canonical.eml`   well-formed RFC 3464 three-part report.
//! - `exchange-wrapped.eml`    `Diagnostic-Code:` wrapped across
//!                             continuation lines (real Exchange).
//! - `multi-recipient.eml`     one DSN reporting multiple failures.
//! - `no-rfc822-part.eml`      DSN with delivery-status but no
//!                             embedded original message.
//! - `sendmail-legacy.eml`     pre-RFC 3464, plain-text only, no
//!                             structured part at all.
//! - `status-only.eml`         delivery-status with `Status:` but no
//!                             `Diagnostic-Code:`.
//! - `soft-bounce-4xx.eml`     4.x.x transient failure — parser
//!                             succeeds, classifier should NOT
//!                             auto-suppress.
//! - `policy-5_7_1.eml`        5.7.1 policy rejection — parser
//!                             succeeds, classifier carves it out
//!                             from auto-suppression.
//!
//! The test asserts two things for every fixture:
//!   1. Parsing never panics.
//!   2. The parser's verdict matches a hand-coded expectation
//!      (`Some(report)` with specific fields, or `None`).
//!
//! When adding a new fixture, append a row to `FIXTURES` rather than
//! writing a new test fn so the corpus stays a single matrix.

use std::path::Path;

use backend::services::channels::bounce_parser::{parse_bounce, BounceReport};

/// Per-fixture expectation. `expected_report` of `None` means the
/// parser should return `None` (DSN was unparseable or carries no
/// linkage info we can extract).
struct Fixture {
    name: &'static str,
    expected: Option<ExpectedReport>,
}

/// Subset of the report fields the test pins; `recipient` and
/// `diagnostic` are checked when `Some`, ignored when `None` so we
/// don't have to nail every detail to make a test informative.
struct ExpectedReport {
    original_message_id: &'static str,
    recipient: Option<&'static str>,
    /// Substring expected to appear in the diagnostic. We don't
    /// pin the full string because MTAs add their own prefixes
    /// (`smtp; ...`, trailing semicolons, line-wraps) which would
    /// make the test brittle without testing anything real.
    diagnostic_contains: Option<&'static str>,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "postfix-canonical.eml",
        expected: Some(ExpectedReport {
            original_message_id: "out-42-canonical@yourco.com",
            recipient: Some("bob@example.org"),
            diagnostic_contains: Some("User unknown"),
        }),
    },
    Fixture {
        name: "exchange-wrapped.eml",
        expected: Some(ExpectedReport {
            original_message_id: "out-100-exch@yourco.com",
            recipient: Some("jane@example.org"),
            // Exchange wraps Diagnostic-Code across continuation
            // lines; our parser keeps only the first line for now,
            // which is what most other libraries do too. The
            // important thing is the first line carries the SMTP
            // code, so classification still works.
            diagnostic_contains: Some("RESOLVER.ADR.RecipientNotFound"),
        }),
    },
    Fixture {
        name: "multi-recipient.eml",
        expected: Some(ExpectedReport {
            original_message_id: "out-multi@yourco.com",
            // We pick up the first Final-Recipient block; the
            // current parser returns the first match. Future
            // improvement: return a Vec<BounceReport> and feed all.
            recipient: Some("alice@example.org"),
            diagnostic_contains: Some("No such user"),
        }),
    },
    Fixture {
        name: "no-rfc822-part.eml",
        // Without an embedded original message we can't recover the
        // outbound Message-ID, so the parser correctly returns None.
        // The pipeline still short-circuits as SkippedBounce, just
        // without the outbound-row linkage.
        expected: None,
    },
    Fixture {
        name: "sendmail-legacy.eml",
        // Plain-text DSN with no `multipart/report` envelope at all;
        // the parser walks the MIME tree and finds nothing useful.
        expected: None,
    },
    Fixture {
        name: "status-only.eml",
        expected: Some(ExpectedReport {
            original_message_id: "out-77-statusonly@yourco.com",
            recipient: Some("somebody@example.org"),
            // No Diagnostic-Code: the parser falls back to Status:.
            diagnostic_contains: Some("5.2.1"),
        }),
    },
    Fixture {
        name: "soft-bounce-4xx.eml",
        // Parser doesn't gate on hard vs soft — that's the
        // classifier's job. So even 4xx DSNs return a report.
        expected: Some(ExpectedReport {
            original_message_id: "out-soft@yourco.com",
            recipient: Some("backed-up@example.org"),
            diagnostic_contains: Some("Mailbox full"),
        }),
    },
    Fixture {
        name: "policy-5_7_1.eml",
        // Parser surfaces the policy rejection just like any other
        // bounce; the suppression carve-out for 5.7.x happens in
        // pipeline::is_hard_bounce, not here.
        expected: Some(ExpectedReport {
            original_message_id: "out-policy@yourco.com",
            recipient: Some("valid@example.org"),
            diagnostic_contains: Some("content restrictions"),
        }),
    },
];

fn load_fixture(name: &str) -> Vec<u8> {
    // CARGO_MANIFEST_DIR points at backend/, so the fixture path is
    // just tests/fixtures/dsn/<name>.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/dsn")
        .join(name);
    std::fs::read(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e))
}

/// Parametric: runs every fixture through the parser and confirms the
/// expectation. A single failure prints which fixture broke so the
/// reader doesn't have to bisect the corpus.
#[test]
fn dsn_corpus_parses_to_expectations() {
    let mut failures: Vec<String> = Vec::new();

    for fixture in FIXTURES {
        let raw = load_fixture(fixture.name);
        let parsed = mailparse::parse_mail(&raw)
            .unwrap_or_else(|e| panic!("fixture {} failed to mailparse: {}", fixture.name, e));
        let report = parse_bounce(&parsed);

        match (&fixture.expected, &report) {
            (None, None) => continue,
            (Some(_), None) => {
                failures.push(format!(
                    "{}: expected Some(report), got None",
                    fixture.name
                ));
            }
            (None, Some(r)) => {
                failures.push(format!(
                    "{}: expected None, got Some({})",
                    fixture.name, r.original_message_id
                ));
            }
            (Some(expected), Some(got)) => {
                if let Err(msg) = check_expectation(expected, got) {
                    failures.push(format!("{}: {}", fixture.name, msg));
                }
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "DSN corpus has {} failure(s):\n  - {}",
            failures.len(),
            failures.join("\n  - ")
        );
    }
}

fn check_expectation(expected: &ExpectedReport, got: &BounceReport) -> Result<(), String> {
    if got.original_message_id != expected.original_message_id {
        return Err(format!(
            "message-id mismatch: expected {:?}, got {:?}",
            expected.original_message_id, got.original_message_id
        ));
    }
    if let Some(want_recipient) = expected.recipient {
        match got.recipient.as_deref() {
            Some(have) if have == want_recipient => {}
            other => {
                return Err(format!(
                    "recipient mismatch: expected {:?}, got {:?}",
                    want_recipient, other
                ));
            }
        }
    }
    if let Some(want_substr) = expected.diagnostic_contains {
        match got.diagnostic.as_deref() {
            Some(have) if have.contains(want_substr) => {}
            other => {
                return Err(format!(
                    "diagnostic mismatch: expected substring {:?}, got {:?}",
                    want_substr, other
                ));
            }
        }
    }
    Ok(())
}

/// Belt-and-braces: every fixture must at least round-trip through
/// `mailparse::parse_mail` without panicking. Catches fixture corpus
/// rot (someone edits a file and breaks its MIME structure).
#[test]
fn dsn_corpus_is_well_formed() {
    for fixture in FIXTURES {
        let raw = load_fixture(fixture.name);
        let result = mailparse::parse_mail(&raw);
        assert!(
            result.is_ok(),
            "fixture {} is not parseable as MIME: {:?}",
            fixture.name,
            result.err()
        );
    }
}
