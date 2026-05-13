//! HMAC-signed image proxy for inbound email rendering.
//!
//! Agents reading an inbound email shouldn't have their IP, user-
//! agent, and read-time leaked back to the sender via tracking
//! pixels. Routing every remote image through our backend hides
//! all of that behind the helpdesk's egress and lets us add
//! abuse defences (size caps, content-type validation, DNS
//! paranoia) the agent's browser doesn't have.
//!
//! ## Security model
//!
//! Two layers of defence stop the proxy from being weaponised:
//!
//!   1. **HMAC URL signing.** The sanitiser at ingest writes
//!      `/api/image-proxy/{sig}/{base64url(url)}` for every
//!      remote image, where `sig` is the first 16 bytes of
//!      `HMAC-SHA256(key, url)` hex-encoded. Without the key
//!      nobody can mint URLs that the proxy will fetch, so the
//!      proxy can't be used as a generic SSRF / amplification
//!      vector by an outside attacker.
//!   2. **DNS-paranoid fetch via [`utils::safe_http`].** Even
//!      with a valid signature (an attacker with DB write
//!      access, or a future bug in the signing flow), the
//!      resolver refuses to connect to RFC1918 / loopback /
//!      link-local / reserved ranges. This is the same client
//!      that AUD-003 deployed for webhook delivery and plugin
//!      registry; reusing it means there's one chokepoint to
//!      audit.
//!
//! Content-Type is validated against `image/*` before any body
//! bytes reach the agent's browser. Size capped at 5 MiB to
//! cover the realistic upper bound of a high-DPI marketing
//! image without giving an attacker an amplification primitive.
//!
//! ## Signing key derivation
//!
//! Rather than adding another mandatory env var, the signing
//! key is derived from `JWT_SECRET` with a fixed domain-separator
//! prefix. Operators already manage `JWT_SECRET`; deriving keeps
//! the surface area minimal. The `v1` suffix in the domain
//! separator gives us room to rotate the derivation later
//! without invalidating *every* baked-in URL (a future
//! migration could re-sign on read).

use std::time::Duration;

use actix_web::{web, HttpResponse, Responder};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use once_cell::sync::Lazy;
use ring::digest;
use ring::hmac;
use tracing::warn;

use crate::utils::safe_http;

/// Hard cap on proxied image body size. Marketing emails ship
/// surprisingly large hero images at 2x DPI; 5 MiB covers the
/// real-world upper bound while still bounding the amplification
/// factor on a hostile URL that the attacker tricks into our
/// signing pipeline (e.g. via a future signing bug).
const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;

/// Per-fetch timeout. Tracking pixels typically respond in tens
/// of milliseconds; legitimate marketing imagery from a CDN
/// rarely exceeds a second. 10s is generous enough to tolerate a
/// slow CDN without becoming a per-comment liveness liability.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Length of the hex-encoded signature in proxy URLs. The
/// underlying HMAC is SHA-256 (32 bytes); we truncate to the
/// first 16 bytes = 32 hex chars before encoding so URLs stay
/// short. 128 bits of entropy is well above the threshold where
/// brute-forcing a single URL is feasible.
const SIG_HEX_LEN: usize = 32;

/// HMAC key derived once from `JWT_SECRET` at first use. Cached
/// in a `Lazy` so signing and verification both hit the same
/// key without re-deriving per request. If `JWT_SECRET` rotates
/// the process restarts, which discards this cache.
static SIGNING_KEY: Lazy<hmac::Key> = Lazy::new(|| {
    let jwt = std::env::var("JWT_SECRET").unwrap_or_default();
    // Domain-separator prefix: tag this derivation as the
    // image-proxy key, never let it collide with another use of
    // `JWT_SECRET`. The trailing version suffix lets us rotate
    // the derivation without changing the env var.
    let mut ctx = digest::Context::new(&digest::SHA256);
    ctx.update(b"nosdesk:image-proxy:v1\0");
    ctx.update(jwt.as_bytes());
    let derived = ctx.finish();
    hmac::Key::new(hmac::HMAC_SHA256, derived.as_ref())
});

/// Build a same-origin proxy URL for an upstream image URL. The
/// signature locks the URL to the proxy: changing the encoded
/// URL by even one byte invalidates the signature, so the
/// proxy can't be redirected to a different host than the
/// sanitiser approved.
///
/// Returned path is just `/api/image-proxy/...`. The caller
/// usually wants this verbatim in an `<img src>` attribute.
pub fn sign_proxy_url(url: &str) -> String {
    let tag = hmac::sign(&SIGNING_KEY, url.as_bytes());
    let sig_hex = hex_encode_truncated(tag.as_ref(), SIG_HEX_LEN / 2);
    let encoded = URL_SAFE_NO_PAD.encode(url.as_bytes());
    format!("/api/image-proxy/{sig_hex}/{encoded}")
}

/// Verify a signature against a candidate URL. Compare runs in
/// constant time via an XOR accumulate so timing side-channels
/// can't be used to forge signatures byte-by-byte.
///
/// `ring::hmac::verify` would do this for us, but we sign with a
/// truncated tag (first 16 bytes of HMAC-SHA256, see SIG_HEX_LEN),
/// and ring's verify requires the full 32-byte output. The XOR
/// loop below is the standard pattern for constant-time bytewise
/// equality on a fixed-length slice.
fn verify_signature(sig_hex: &str, url: &str) -> bool {
    if sig_hex.len() != SIG_HEX_LEN {
        return false;
    }
    let Some(sig_bytes) = hex_decode(sig_hex) else {
        return false;
    };
    let expected_full = hmac::sign(&SIGNING_KEY, url.as_bytes());
    let expected = &expected_full.as_ref()[..SIG_HEX_LEN / 2];
    if sig_bytes.len() != expected.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (a, b) in sig_bytes.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn hex_encode_truncated(bytes: &[u8], truncate_to: usize) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let take = truncate_to.min(bytes.len());
    let mut out = String::with_capacity(take * 2);
    for &b in &bytes[..take] {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for chunk in hex.as_bytes().chunks(2) {
        let hi = hex_digit(chunk[0])?;
        let lo = hex_digit(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `GET /api/image-proxy/{sig}/{encoded_url}`.
///
/// Path-positional rather than query-string so a misbehaving log
/// scrubber that strips `?` params doesn't break attribution.
/// Returns 404 for any verification or fetch failure — the
/// proxy mustn't reveal *why* a particular URL failed (signature
/// mismatch vs upstream error vs blocked content type), since
/// the differential could help an attacker probe the signing
/// key boundary.
pub async fn proxy_image(path: web::Path<(String, String)>) -> impl Responder {
    let (sig_hex, encoded_url) = path.into_inner();

    let Some(url_bytes) = URL_SAFE_NO_PAD.decode(&encoded_url).ok() else {
        return HttpResponse::NotFound().finish();
    };
    let Ok(url) = std::str::from_utf8(&url_bytes) else {
        return HttpResponse::NotFound().finish();
    };

    if !verify_signature(&sig_hex, url) {
        // Don't log the URL — a torrent of these would be log
        // noise on a busy mail flow with backfill in progress.
        return HttpResponse::NotFound().finish();
    }

    // Belt-and-braces: refuse IP-literal URLs at the URL layer
    // before the resolver even gets involved. `safe_http`'s
    // resolver also catches these, but checking here keeps the
    // failure mode consistent and is cheap.
    if safe_http::reject_unsafe_ip_literal(url).is_err() {
        return HttpResponse::NotFound().finish();
    }

    let client = match safe_http::client(FETCH_TIMEOUT) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "image proxy: failed to build safe_http client");
            return HttpResponse::ServiceUnavailable().finish();
        }
    };

    let response = match client.get(url).send().await {
        Ok(r) => r,
        Err(_) => {
            // Connection refused, DNS-paranoid resolver said no,
            // upstream TLS failure — all map to a generic 404.
            return HttpResponse::NotFound().finish();
        }
    };

    if !response.status().is_success() {
        return HttpResponse::NotFound().finish();
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    // The proxy is for raster images only. Refuse non-image
    // content-types: a misbehaving upstream that returns text/html
    // when we asked for an image could render in some clients.
    // SVG is blocked specifically because it can carry inline
    // <script> and is an XSS surface (same reasoning as AUD-006's
    // upload-side SVG block); raster image MIME types only.
    let ct_lower = content_type.to_ascii_lowercase();
    if !ct_lower.starts_with("image/") || ct_lower.starts_with("image/svg") {
        return HttpResponse::NotFound().finish();
    }

    // Early reject by Content-Length when the upstream supplies
    // it. Saves the bandwidth of a chunked download we'd just
    // discard. A hostile upstream that lies about Content-Length
    // is still caught by the per-chunk accumulation below.
    if let Some(declared) = response.content_length() {
        if declared as usize > MAX_BODY_BYTES {
            return HttpResponse::NotFound().finish();
        }
    }

    // Stream the body chunk-by-chunk so a hostile upstream
    // streaming 100GB while claiming `Content-Type: image/png`
    // can't OOM us: the running total is checked per chunk and
    // we drop the connection as soon as it crosses the cap.
    // `response.bytes()` would buffer the entire response in
    // memory before the size check could run, which is the OOM
    // surface this loop closes.
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut stream = response;
    loop {
        let chunk = match stream.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(_) => return HttpResponse::NotFound().finish(),
        };
        if buf.len().saturating_add(chunk.len()) > MAX_BODY_BYTES {
            return HttpResponse::NotFound().finish();
        }
        buf.extend_from_slice(&chunk);
    }

    HttpResponse::Ok()
        .insert_header(("Content-Type", content_type))
        // Aggressive cache: proxied images are content-addressed
        // by the upstream URL, and the URL is in the path, so two
        // identical fetches will hit the browser cache rather
        // than the proxy. 1 day matches Camo's default.
        .insert_header(("Cache-Control", "public, max-age=86400, immutable"))
        // Defence in depth against an upstream redirect to an
        // inline-renderable type.
        .insert_header(("X-Content-Type-Options", "nosniff"))
        .body(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_test_jwt<F: FnOnce()>(f: F) {
        // Each test sets JWT_SECRET to a known value so the
        // derived signing key is deterministic. We don't restore
        // it after — these tests don't run alongside anything
        // that reads JWT_SECRET, and the cached SIGNING_KEY is
        // computed once per process anyway.
        std::env::set_var(
            "JWT_SECRET",
            "test-secret-for-image-proxy-signing-deterministic",
        );
        f();
    }

    #[test]
    fn sign_then_verify_roundtrip() {
        with_test_jwt(|| {
            let url = "https://example.com/path/to/image.png?v=1";
            let proxy = sign_proxy_url(url);
            assert!(proxy.starts_with("/api/image-proxy/"));
            // Split path: /api/image-proxy/{sig}/{encoded}
            let parts: Vec<&str> = proxy.splitn(5, '/').collect();
            let sig = parts[3];
            assert!(verify_signature(sig, url));
        });
    }

    #[test]
    fn signature_locks_the_url() {
        with_test_jwt(|| {
            let url1 = "https://example.com/a.png";
            let url2 = "https://example.com/b.png";
            let proxy = sign_proxy_url(url1);
            let parts: Vec<&str> = proxy.splitn(5, '/').collect();
            let sig = parts[3];
            assert!(verify_signature(sig, url1));
            assert!(
                !verify_signature(sig, url2),
                "signature for url1 must not validate url2"
            );
        });
    }

    #[test]
    fn malformed_signature_rejected() {
        with_test_jwt(|| {
            let url = "https://example.com/a.png";
            assert!(!verify_signature("", url));
            assert!(!verify_signature("not-hex-at-all-but-32-chars-abcde", url));
            assert!(!verify_signature("aabbccdd", url)); // too short
        });
    }

    #[test]
    fn hex_encode_truncated_pads_correctly() {
        let bytes = &[0xab, 0xcd, 0xef, 0x01];
        assert_eq!(hex_encode_truncated(bytes, 4), "abcdef01");
        assert_eq!(hex_encode_truncated(bytes, 2), "abcd");
        // Asking for more than we have caps at the input length.
        assert_eq!(hex_encode_truncated(bytes, 10), "abcdef01");
    }
}
