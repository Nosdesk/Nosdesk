//! Lint: handlers build error responses through `crate::errors`, never a raw
//! `HttpResponse::<4xx/5xx>()` constructor.
//!
//! The builders are the one place the wire contract (`{error, code}` plus
//! optional diagnostic fields) and the `ErrorKind` stamp the canonical request
//! event reads are defined. A raw constructor sidesteps both: the body drifts
//! (a bare `.finish()`, `"message"` instead of `"error"`, an internal error
//! string echoed to the client) and the request event reports no
//! `error_kind`. `errors::with_fields` covers bodies that need extra keys;
//! `errors::from_response` carries one through `actix_web::Error`.
//!
//! The one exemption is listed with its reason and checked in both directions,
//! so it cannot rot into an amnesty.
//!
//! Only the named status constructors are matched. `HttpResponse::build(status)`
//! with a dynamic status is not, because the same form builds 2xx responses;
//! the two error sites that used it were converted by hand
//! (`errors::with_fields` takes a `StatusCode`).

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Files under `src/handlers` allowed to build a 4xx/5xx by hand, and why.
const EXEMPT: &[(&str, &str)] = &[(
    "health.rs",
    "probe endpoints answer 503 with a component status report, not an error body",
)];

fn handler_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read handlers dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            handler_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn handlers_build_errors_through_the_builders() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/handlers");
    let raw = Regex::new(
        r"HttpResponse::(BadRequest|Unauthorized|PaymentRequired|Forbidden|NotFound|MethodNotAllowed|NotAcceptable|Conflict|Gone|PayloadTooLarge|UnsupportedMediaType|UnprocessableEntity|TooManyRequests|InternalServerError|NotImplemented|BadGateway|ServiceUnavailable|GatewayTimeout)\(\)",
    )
    .unwrap();

    let mut files = Vec::new();
    handler_files(&root, &mut files);
    files.sort();

    let mut offences = Vec::new();
    let mut exempt_hit = vec![false; EXEMPT.len()];
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let src = fs::read_to_string(path).unwrap();
        let hits: Vec<usize> = src
            .lines()
            .enumerate()
            .filter(|(_, l)| raw.is_match(l))
            .map(|(i, _)| i + 1)
            .collect();
        if hits.is_empty() {
            continue;
        }
        if let Some(i) = EXEMPT.iter().position(|(f, _)| *f == rel) {
            exempt_hit[i] = true;
            continue;
        }
        for line in hits {
            offences.push(format!("  src/handlers/{rel}:{line}"));
        }
    }

    assert!(
        offences.is_empty(),
        "raw HttpResponse::<4xx/5xx>() in handlers; use the crate::errors builders \
         (errors::with_fields for bodies that need extra keys):\n{}",
        offences.join("\n")
    );
    for (i, (file, reason)) in EXEMPT.iter().enumerate() {
        assert!(
            exempt_hit[i],
            "{file} is exempt ({reason}) but has no raw constructor left; remove the exemption"
        );
    }
}
