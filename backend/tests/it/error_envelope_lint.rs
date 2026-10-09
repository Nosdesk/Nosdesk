//! Lint: every API error answers with the `{error, code}` envelope.
//!
//! `handler_error_builder_lint` keeps raw `HttpResponse::<4xx/5xx>()` out of
//! `src/handlers`. Two more ways around the envelope are checked here:
//!
//! - `actix_web::error::Error*` helpers (`ErrorNotFound("...")` and the
//!   rest) anywhere in `src/`, called by path or imported
//!   (`use actix_web::error::{ErrorBadRequest, ...}`): their body is plain
//!   text with no code. Use the `errors::*_error` builders, which return an
//!   `actix_web::Error` carrying the envelope. The files in `PLAIN_TEXT_OK`
//!   are exempt, each with its reason, checked in both directions.
//! - raw `HttpResponse::<4xx/5xx>()` in `src/extractors` and
//!   `src/middleware`, whose refusals reach every route.
//!
//! Test modules (from the first `#[cfg(test)] mod`) are skipped.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Files under `src/` allowed to answer in plain text, and why.
const PLAIN_TEXT_OK: &[(&str, &str)] = &[(
    "handlers/unsubscribe.rs",
    "the one-click unsubscribe GET is a page a person opens from an email, not an API call",
)];

/// An `actix_web::error::Error*` helper brought in by `use`.
const IMPORTED_HELPER: &str = r"use actix_web::error::(\{[^}]*\bError[A-Z]|Error[A-Z])";

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The file's lines up to its test module (`#[cfg(test)]` followed by a
/// `mod`), leaving out comment lines. A `#[cfg(test)]` on anything else
/// (a helper, an impl) doesn't end the scan.
fn non_test_lines(path: &Path) -> Vec<(usize, String)> {
    let src = fs::read_to_string(path).expect("read source");
    let lines: Vec<&str> = src.lines().collect();
    let end = (0..lines.len())
        .find(|&i| {
            lines[i].trim() == "#[cfg(test)]"
                && lines[i + 1..]
                    .iter()
                    .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with("#["))
                    .is_some_and(|l| l.trim_start().starts_with("mod "))
        })
        .unwrap_or(lines.len());
    lines[..end]
        .iter()
        .enumerate()
        .filter(|(_, l)| !l.trim_start().starts_with("//"))
        .map(|(i, l)| (i + 1, l.to_string()))
        .collect()
}

#[test]
fn api_errors_carry_the_error_envelope() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let plain = Regex::new(r"\berror::Error[A-Z][A-Za-z]*\(").unwrap();
    let imported = Regex::new(IMPORTED_HELPER).unwrap();
    let raw = Regex::new(
        r"HttpResponse::(BadRequest|Unauthorized|PaymentRequired|Forbidden|NotFound|MethodNotAllowed|NotAcceptable|Conflict|Gone|PayloadTooLarge|UnsupportedMediaType|UnprocessableEntity|TooManyRequests|InternalServerError|NotImplemented|BadGateway|ServiceUnavailable|GatewayTimeout)\(\)",
    )
    .unwrap();

    let mut files = Vec::new();
    rust_files(&root, &mut files);
    files.sort();

    let mut offences = Vec::new();
    let mut exempt_hit = vec![false; PLAIN_TEXT_OK.len()];
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        if rel == "errors.rs" {
            continue;
        }
        let exempt = PLAIN_TEXT_OK.iter().position(|(f, _)| *f == rel);
        let edge = rel.starts_with("extractors/") || rel.starts_with("middleware/");
        for (line, text) in non_test_lines(path) {
            if plain.is_match(&text) || imported.is_match(&text) {
                match exempt {
                    Some(i) => exempt_hit[i] = true,
                    None => offences.push(format!("  src/{rel}:{line} plain-text actix error")),
                }
            }
            if edge && raw.is_match(&text) {
                offences.push(format!("  src/{rel}:{line} raw error response"));
            }
        }
    }
    for (i, (file, _)) in PLAIN_TEXT_OK.iter().enumerate() {
        assert!(
            exempt_hit[i],
            "src/{file} no longer answers in plain text: drop it from PLAIN_TEXT_OK"
        );
    }
    assert!(
        offences.is_empty(),
        "API errors must use the `{{error, code}}` envelope (crate::errors builders):\n{}",
        offences.join("\n")
    );
}

#[test]
fn the_import_rule_sees_an_imported_helper() {
    let imported = Regex::new(IMPORTED_HELPER).unwrap();
    assert!(imported.is_match("use actix_web::error::{ErrorBadRequest, ErrorInternalServerError};"));
    assert!(imported.is_match("use actix_web::error::ErrorNotFound;"));
    assert!(!imported.is_match("use actix_web::error::{JsonPayloadError, PathError};"));
}
