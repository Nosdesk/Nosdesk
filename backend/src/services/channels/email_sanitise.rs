//! HTML sanitiser for inbound email comments.
//!
//! Two-pass pipeline run at ingest:
//!
//!   1. **Outlook pre-strip** (regex) — removes the Word / Outlook
//!      junk that html5ever doesn't parse out for us: namespaced
//!      `<o:p>` and `<v:*>` tags, `xmlns:v` / `xmlns:o` attribute
//!      noise, and `mso-*` CSS properties hiding inside `style=`
//!      values. Conditional comments (`<!--[if mso]>...<![endif]-->`)
//!      are handled by html5ever's comment node stripping, so they
//!      don't need their own pass.
//!
//!   2. **`ammonia` HTML sanitisation** — strict tag and attribute
//!      allowlist, hostile URL schemes rejected, event handlers
//!      stripped. Built on html5ever so the parse is real, not
//!      regex-based.
//!
//! Output lands in `comments.new_content` + `comments.quoted_content`
//! at ingest time (sanitisation now runs BEFORE the quote split,
//! so concatenating the two persisted columns equals the full
//! sanitised body — the previous standalone `sanitised_html`
//! column was dropped in 2026-05-13-120000). The frontend renders
//! the result inside a sandboxed iframe with an inline CSP and
//! runs DOMPurify before injection as defence-in-depth. Re-
//! sanitisation on policy change is a backfill that re-reads
//! `raw_source_uri` and runs this module again; no upstream
//! re-fetch needed.
//!
//! **What this module deliberately does not do yet:**
//!
//!   - CSS property allowlist beyond `mso-*` stripping. A full
//!     allowlist (block `position`, transforms, `@import`, cap
//!     `font-size`) is the natural Pass 2.x follow-up via
//!     `lightningcss`. Until then the style attribute survives
//!     ammonia with `mso-*` removed, which is the simplest
//!     reasonable default.
//!   - Remote image rewriting and tracker blocklists. These are
//!     Pass 3 work; the iframe-level CSP `img-src` directive
//!     blocks remote loads in the meantime.
//!   - CID inline-image extraction. Requires MIME-part access at
//!     sanitise time, which is its own threading concern. Pass
//!     2.x follow-up.

use once_cell::sync::Lazy;
use regex::Regex;

/// Outcome of sanitising one inbound email body. Returned as a
/// struct rather than a bare String so future passes can add
/// fields (inline_images, remote_images_blocked) without breaking
/// callers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SanitisedEmail {
    /// Render-ready HTML. Safe to inject into a sandboxed iframe's
    /// `srcdoc`; the frontend should still run DOMPurify on it
    /// for defence-in-depth.
    pub html: String,
    /// Human-readable names of tracker services whose pixel
    /// images were stripped from the body. Powers the "Stripped
    /// tracker from Mailchimp" attribution affordance. Empty
    /// when nothing was stripped. Names are deduplicated so the
    /// UI doesn't repeat a brand if the same sender embedded
    /// multiple pixels.
    pub trackers_stripped: Vec<String>,
}

/// Sanitise one HTML email body for ticket-detail rendering.
///
/// Idempotent: running the sanitiser on its own output is a
/// no-op, which matters for the backfill flow where stored
/// `sanitised_html` may be re-sanitised after a policy change.
pub fn sanitise(raw_html: &str) -> SanitisedEmail {
    let pre = outlook_pre_strip(raw_html);
    let clean = ammonia_clean(&pre);
    let (after_trackers, trackers_stripped) = strip_known_trackers(&clean);
    let final_html = rewrite_remote_images_to_proxy(&after_trackers);
    SanitisedEmail {
        html: final_html,
        trackers_stripped,
    }
}

/// Rewrite every `<img src="http(s)://...">` in the HTML to point
/// at the same-origin image proxy. The proxy signs the upstream
/// URL with HMAC-SHA256 derived from `JWT_SECRET`, so the URL we
/// emit here is locked to this specific upstream and can't be
/// repurposed. After this pass the iframe CSP can tighten
/// `img-src` to `'self' data: cid:` because every remote image is
/// now same-origin.
///
/// cid: and data: URLs are passed through untouched — they don't
/// reach the network so the proxy adds nothing. Idempotency:
/// the regex requires `http(s)://` so an already-proxied URL
/// (which is `/api/image-proxy/...`) won't match a second time.
fn rewrite_remote_images_to_proxy(html: &str) -> String {
    IMG_TAG_RE
        .replace_all(html, |caps: &regex::Captures<'_>| {
            let original_src = &caps["src"];
            // Only http(s) gets proxied. cid:, data:, and relative
            // URLs don't reach the network and don't need it.
            let lower = original_src.to_ascii_lowercase();
            if !lower.starts_with("http://") && !lower.starts_with("https://") {
                return caps[0].to_string();
            }
            let proxy_url = crate::handlers::image_proxy::sign_proxy_url(original_src);
            // Replace the src= attribute value in-place, preserving
            // every other attribute on the img tag. The matched
            // text has the shape `<img ... src="ORIGINAL" ...>`
            // (with attribute order arbitrary); we find the src
            // group's range in the match and splice in the proxy
            // URL.
            let full = &caps[0];
            let src_match = caps.name("src").expect("src group always present");
            // Offsets are relative to `html`; convert to in-`full`.
            let match_start = caps.get(0).unwrap().start();
            let local_start = src_match.start() - match_start;
            let local_end = src_match.end() - match_start;
            let mut out = String::with_capacity(full.len() + proxy_url.len());
            out.push_str(&full[..local_start]);
            out.push_str(&proxy_url);
            out.push_str(&full[local_end..]);
            out
        })
        .into_owned()
}

/// Drop the Word / Outlook junk that html5ever would otherwise
/// keep in the parse tree. Operates on the source string rather
/// than the DOM because the targeted patterns are simpler to
/// match textually and we hand off to ammonia (which re-parses)
/// immediately after.
fn outlook_pre_strip(html: &str) -> String {
    let stripped_tags = OUTLOOK_TAG_RE.replace_all(html, "");
    let stripped_attrs = OUTLOOK_NS_ATTR_RE.replace_all(&stripped_tags, "");
    strip_mso_css(&stripped_attrs)
}

/// Strip `<o:p>...</o:p>`, `<o:p/>`, and any `<v:foo ...>...</v:foo>`
/// pair or self-close. The body text inside these elements is
/// rarely meaningful (Outlook uses them as layout shims), but we
/// keep text content where present by stripping only the tag
/// markers, not the text between.
static OUTLOOK_TAG_RE: Lazy<Regex> = Lazy::new(|| {
    // Match opening, closing, or self-closing namespaced tags for
    // `o:*` and `v:*`. `(?is)` = case-insensitive + `.` matches
    // newline, so attribute values can span lines. We *remove the
    // tag markers* but keep any text content between an opener
    // and its closer; this is a textual transform, not a DOM
    // walk, but it's good enough for the Word-emitted junk we're
    // targeting.
    Regex::new(r"(?is)</?(?:o|v):[a-z0-9_-]+(?:\s[^>]*)?/?>").expect("valid Outlook tag regex")
});

/// Strip `xmlns:v="..."` / `xmlns:o="..."` namespace attributes.
/// These can survive ammonia's allowlist as unknown-but-harmless
/// attributes; cleaner to drop them at the pre-strip stage so the
/// final HTML is uncluttered.
static OUTLOOK_NS_ATTR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)\s+xmlns:(?:o|v|w|m)\s*=\s*"[^"]*""#).expect("valid Outlook xmlns regex")
});

/// Remove `mso-*` CSS properties from any `style="..."` attribute.
/// Walks each style value, splits on `;`, drops properties whose
/// name (left of `:`) starts with `mso-` (case-insensitively),
/// and rejoins. Leaves other properties untouched.
fn strip_mso_css(html: &str) -> String {
    STYLE_ATTR_RE
        .replace_all(html, |caps: &regex::Captures<'_>| {
            // Either group 1 (double-quoted) or group 2 (single-
            // quoted) holds the style value. The regex guarantees
            // exactly one is populated per match.
            let raw = caps
                .get(1)
                .or_else(|| caps.get(2))
                .map(|m| m.as_str())
                .unwrap_or("");
            let cleaned = raw
                .split(';')
                .filter(|prop| {
                    let name = prop.split(':').next().unwrap_or("").trim();
                    !name.eq_ignore_ascii_case("mso")
                        && !name.to_ascii_lowercase().starts_with("mso-")
                })
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join("; ");
            if cleaned.is_empty() {
                // Drop the entire `style=""` attribute when all its
                // properties were Outlook noise.
                String::new()
            } else {
                // Always emit double-quoted form. Ammonia normalises
                // attribute quoting in its output pass anyway, but
                // canonicalising here keeps the regex round-trip
                // predictable for future maintainers reading the
                // transformed-but-not-yet-ammonia'd intermediate.
                format!(" style=\"{}\"", cleaned)
            }
        })
        .into_owned()
}

/// Capture group 1 holds the contents of a `style="..."` attribute
/// (double-quoted form), group 2 the single-quoted variant. Exactly
/// one of the two is populated per match. Outlook and most other
/// clients emit double quotes, but emails minified through certain
/// pipelines (notably MJML output and a few transactional senders)
/// emit single quotes; covering both keeps the mso-* strip from
/// silently no-oping on those.
static STYLE_ATTR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)\s+style\s*=\s*(?:"([^"]*)"|'([^']*)')"#)
        .expect("valid style attribute regex")
});

/// Walk every `<img>` tag in the ammonia-cleaned HTML, parse its
/// `src` URL, and check the host against the tracker blocklist.
/// Matched tags are dropped (the entire `<img ...>` element
/// disappears), and the tracker's display name is recorded so the
/// renderer can show "Stripped tracker from X" attribution.
///
/// Operates on the post-ammonia string with a regex because
/// ammonia's output is well-formed: `<img>` is always
/// `<img attr="value" attr="value" ...>`, attributes are
/// double-quoted, and tags don't span newlines in surprising
/// ways. A real DOM walk would be a larger dep for marginal
/// correctness gain.
fn strip_known_trackers(html: &str) -> (String, Vec<String>) {
    let mut stripped = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let replaced = IMG_TAG_RE.replace_all(html, |caps: &regex::Captures<'_>| {
        let src = &caps["src"];
        let Some(host) = extract_host(src) else {
            // Unparseable URL: keep the img as-is rather than
            // silently dropping legitimate-looking content.
            return caps[0].to_string();
        };
        if let Some(name) = super::email_trackers::match_tracker(&host) {
            // First sighting of this tracker brand records the
            // name; subsequent occurrences from the same brand
            // get dropped silently to avoid "stripped 14
            // Mailchimp trackers" attribution noise.
            if seen.insert(name) {
                stripped.push(name.to_string());
            }
            String::new()
        } else {
            caps[0].to_string()
        }
    });

    (replaced.into_owned(), stripped)
}

/// Capture group `src` holds the URL inside an `<img src="...">`
/// tag. Anchored to `<img` and the closing `>` so we don't match
/// a `src=` attribute on some other element.
static IMG_TAG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?is)<img\b[^>]*\bsrc\s*=\s*"(?P<src>[^"]*)"[^>]*>"#)
        .expect("valid img tag regex")
});

/// Extract the host component of a URL. Accepts only `http(s)://`
/// URLs; CID refs, data URLs, and anything else return `None`
/// because they aren't reachable by remote trackers anyway.
fn extract_host(url: &str) -> Option<String> {
    let after_scheme = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .or_else(|| url.strip_prefix("HTTP://"))
        .or_else(|| url.strip_prefix("HTTPS://"))?;
    // Authority section runs up to the first /, ?, or #.
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    // Strip optional userinfo (user:pass@) and port (:80).
    let host_start = authority.rfind('@').map(|i| i + 1).unwrap_or(0);
    let host_with_port = &authority[host_start..];
    let host_end = host_with_port.find(':').unwrap_or(host_with_port.len());
    let host = &host_with_port[..host_end];
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// Run ammonia with the email-rendering allowlist. The default
/// `ammonia::Builder` already strips `<script>`, `<iframe>`,
/// `<object>`, `<embed>`, event handlers, and `javascript:` URLs.
/// We tighten further:
///
///   - URL schemes restricted to http, https, mailto, cid (for
///     inline-image references that Pass 2.x will rewrite),
///     tel, sms. `data:` is rejected because inline data URLs
///     are an XSS vector when the renderer is permissive about
///     `image/svg+xml`.
///   - `style` attribute kept (Outlook strip ran first) so the
///     visual layout of well-formed marketing emails survives.
///     Pass 2.x's lightningcss pass adds the property allowlist.
///   - `<base>`, `<meta>`, `<link>` removed. The iframe srcdoc
///     supplies its own `<base target="_blank">`; allowing the
///     email body to override that defeats the navigation
///     boundary.
fn ammonia_clean(html: &str) -> String {
    AMMONIA.clean(html).to_string()
}

static AMMONIA: Lazy<ammonia::Builder<'static>> = Lazy::new(|| {
    let mut builder = ammonia::Builder::default();

    // URL schemes: drop `data:` for safety (svg-as-data is the
    // common XSS surface) and keep the rest of ammonia's
    // default-allowed set.
    builder.url_schemes(
        ["http", "https", "mailto", "cid", "tel", "sms"]
            .iter()
            .copied()
            .collect(),
    );

    // `style` isn't in ammonia's default attribute allowlist for
    // any tag, but real-world emails depend on inline styling.
    // Adding it generically (instead of per-tag) keeps the policy
    // readable. The Outlook pre-strip already neutralised the
    // worst Word-derived style values (`mso-*`); a Pass 2.x
    // `lightningcss` pass adds the property-level allowlist
    // (block `position`, transforms, etc.) once we adopt it.
    builder.add_generic_attributes(["style"]);

    // Belt-and-braces tag denials beyond ammonia's defaults.
    // `<base>` and `<meta>` are unlikely in body fragments but
    // would override the iframe's `<base target="_blank">`.
    // `<link>` could load remote stylesheets despite the
    // iframe-level `style-src` CSP — block at the parse stage so
    // it never reaches the browser.
    builder.rm_tags(["base", "meta", "link"]);

    builder
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_outlook_namespaced_tags() {
        let input = "<p>Hello <o:p>x</o:p>world</p>";
        let out = sanitise(input).html;
        assert!(out.contains("Hello "), "should keep visible text");
        assert!(out.contains("world"), "should keep visible text");
        assert!(!out.contains("<o:p"), "should drop o:p opener");
        assert!(!out.contains("</o:p>"), "should drop o:p closer");
    }

    #[test]
    fn drops_outlook_xmlns_attrs() {
        // ammonia will strip the outer html-shell anyway, but we want
        // to ensure the xmlns noise is gone in nested element forms.
        let input = r#"<p xmlns:o="urn:schemas-microsoft-com:office:office"
                         xmlns:v="urn:schemas-microsoft-com:vml">hi</p>"#;
        let out = sanitise(input).html;
        assert!(!out.contains("xmlns:o"), "should drop xmlns:o");
        assert!(!out.contains("xmlns:v"), "should drop xmlns:v");
        assert!(out.contains("hi"));
    }

    #[test]
    fn strips_mso_css_properties_keeps_others() {
        let input = r#"<p style="color: red; mso-margin-top-alt: auto; font-size: 14px; mso-pagination: widow-orphan">hi</p>"#;
        let out = sanitise(input).html;
        assert!(out.contains("color: red"), "non-mso props survive");
        assert!(out.contains("font-size: 14px"), "non-mso props survive");
        assert!(
            !out.to_ascii_lowercase().contains("mso-"),
            "mso-* props are stripped: {out}"
        );
    }

    #[test]
    fn strips_mso_from_single_quoted_style() {
        // Some pipelines (notably MJML output) emit single-quoted
        // attributes. Both quote forms are recognised so the strip
        // doesn't silently no-op on this minority.
        let input = "<p style='color: red; mso-bidi-font-size: 11pt; font-weight: bold'>hi</p>";
        let out = sanitise(input).html;
        assert!(out.contains("color: red"));
        assert!(out.contains("font-weight: bold"));
        assert!(
            !out.to_ascii_lowercase().contains("mso-"),
            "mso-* stripped from single-quoted style: {out}"
        );
    }

    #[test]
    fn drops_style_attribute_when_all_props_are_mso() {
        let input = r#"<p style="mso-margin-top-alt: auto; mso-pagination: widow-orphan">hi</p>"#;
        let out = sanitise(input).html;
        // Result should not carry an empty `style=""` artifact.
        assert!(!out.contains(r#"style="""#), "no empty style attr");
        assert!(!out.contains("style=''"), "no empty style attr");
        assert!(out.contains("hi"));
    }

    #[test]
    fn drops_conditional_comments() {
        // html5ever treats `<!--[if mso]>` as a comment, and ammonia
        // drops comments by default. End-to-end check.
        let input =
            "<p>Visible</p><!--[if mso]><table><tr><td>Outlook only</td></tr></table><![endif]-->";
        let out = sanitise(input).html;
        assert!(out.contains("Visible"));
        assert!(!out.contains("Outlook only"), "conditional content gone");
    }

    #[test]
    fn ammonia_strips_script() {
        let input = "<p>hi</p><script>alert(1)</script>";
        let out = sanitise(input).html;
        assert!(!out.contains("script"), "script tag removed");
        assert!(!out.contains("alert"), "script content removed");
    }

    #[test]
    fn ammonia_strips_event_handlers() {
        let input = r#"<a href="https://example.com" onclick="alert(1)">click</a>"#;
        let out = sanitise(input).html;
        assert!(!out.contains("onclick"));
        assert!(!out.contains("alert"));
        assert!(out.contains("example.com"), "safe attrs preserved");
    }

    #[test]
    fn ammonia_strips_iframe() {
        let input = r#"<p>hi</p><iframe src="https://evil.example"></iframe>"#;
        let out = sanitise(input).html;
        assert!(!out.contains("iframe"));
        assert!(out.contains("hi"));
    }

    #[test]
    fn ammonia_strips_javascript_urls() {
        let input = r#"<a href="javascript:alert(1)">click</a>"#;
        let out = sanitise(input).html;
        assert!(!out.contains("javascript:"));
    }

    #[test]
    fn ammonia_strips_data_urls() {
        // data: URLs (including svg-as-data, the XSS vector) are
        // rejected by the schemes allowlist.
        let input = r#"<img src="data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=" alt="x">"#;
        let out = sanitise(input).html;
        assert!(!out.contains("data:"), "data: URL stripped: {out}");
    }

    #[test]
    fn ammonia_keeps_cid_images() {
        // CID inline images are how mail clients reference attached
        // images; we preserve the reference (Pass 2.x will rewrite
        // the src to a stored URL).
        let input = r#"<img src="cid:abc123@example.com" alt="x">"#;
        let out = sanitise(input).html;
        assert!(out.contains("cid:abc123@example.com"), "cid: kept: {out}");
    }

    #[test]
    fn ammonia_strips_base_meta_link() {
        let input = r#"<base href="https://evil"><meta charset="utf-8"><link rel="stylesheet" href="https://evil/x.css"><p>hi</p>"#;
        let out = sanitise(input).html;
        assert!(!out.contains("<base"));
        assert!(!out.contains("<meta"));
        assert!(!out.contains("<link"));
        assert!(out.contains("hi"));
    }

    #[test]
    fn idempotent_on_clean_input() {
        let input = r#"<p style="color: red">Hi <a href="https://example.com">there</a></p>"#;
        let once = sanitise(input).html;
        let twice = sanitise(&once).html;
        assert_eq!(once, twice, "second pass should be a no-op");
    }

    #[test]
    fn strips_known_tracker_img() {
        let input =
            r#"<p>Hi</p><img src="https://track.mailchimp.com/p.gif?u=abc" width="1" height="1">"#;
        let out = sanitise(input);
        assert!(
            !out.html.to_lowercase().contains("mailchimp"),
            "tracker img dropped: {}",
            out.html
        );
        assert!(out.html.contains("Hi"));
        assert_eq!(out.trackers_stripped, vec!["Mailchimp".to_string()]);
    }

    #[test]
    fn preserves_non_tracker_img_via_proxy() {
        // A non-tracker remote image survives ammonia and the
        // tracker pass, then the rewrite swaps its src for the
        // signed proxy path. The img element and alt attribute
        // stay; only the src changes.
        std::env::set_var(
            "JWT_SECRET",
            "test-secret-for-image-proxy-signing-deterministic",
        );
        let input = r#"<p>Hi</p><img src="https://cdn.example.com/logo.png" alt="Logo">"#;
        let out = sanitise(input);
        // Original upstream gone — the rewrite replaced it.
        assert!(!out.html.contains("cdn.example.com"));
        // Proxy URL is what reaches the renderer.
        assert!(out.html.contains("/api/image-proxy/"));
        // alt attribute preserved.
        assert!(out.html.contains(r#"alt="Logo""#));
        // Not a tracker, so attribution stays empty.
        assert!(out.trackers_stripped.is_empty());
    }

    #[test]
    fn cid_img_kept_and_not_misclassified() {
        // CID references aren't reachable by remote trackers, so
        // the tracker matcher should pass them through.
        let input = r#"<img src="cid:abc@example.com" alt="inline">"#;
        let out = sanitise(input);
        assert!(out.html.contains("cid:abc@example.com"));
        assert!(out.trackers_stripped.is_empty());
    }

    #[test]
    fn multiple_trackers_deduplicated_in_attribution() {
        // A marketing email with three Mailchimp pixels gets all
        // three dropped, but the attribution list says "Mailchimp"
        // once, not three times.
        let input = r#"
            <img src="https://track.mailchimp.com/a.gif">
            <p>Body</p>
            <img src="https://track.mailchimp.com/b.gif">
            <img src="https://list-manage.com/c.gif">
        "#;
        let out = sanitise(input);
        assert!(!out.html.to_lowercase().contains("mailchimp"));
        assert!(out.html.contains("Body"));
        assert_eq!(out.trackers_stripped, vec!["Mailchimp".to_string()]);
    }

    #[test]
    fn rewrites_remote_img_src_to_proxy() {
        // JWT_SECRET must be set for the proxy signer to produce
        // deterministic output across test runs.
        std::env::set_var(
            "JWT_SECRET",
            "test-secret-for-image-proxy-signing-deterministic",
        );
        let input = r#"<p>Hi</p><img src="https://cdn.example.com/banner.png" alt="banner">"#;
        let out = sanitise(input).html;
        // Original upstream URL no longer present (replaced).
        assert!(
            !out.contains("cdn.example.com"),
            "remote src must be rewritten: {out}"
        );
        // Proxy URL is.
        assert!(
            out.contains("/api/image-proxy/"),
            "proxy URL injected: {out}"
        );
        // Other attributes preserved (alt).
        assert!(
            out.contains(r#"alt="banner""#),
            "other img attrs preserved: {out}"
        );
    }

    #[test]
    fn passes_through_cid_images() {
        std::env::set_var(
            "JWT_SECRET",
            "test-secret-for-image-proxy-signing-deterministic",
        );
        let input = r#"<img src="cid:abc@example.com" alt="x">"#;
        let out = sanitise(input).html;
        assert!(
            out.contains("cid:abc@example.com"),
            "cid not rewritten: {out}"
        );
        assert!(!out.contains("/api/image-proxy/"));
    }

    #[test]
    fn rewrite_is_idempotent() {
        // Once an img src points at /api/image-proxy/... the
        // regex requires http(s):// to match again, so a second
        // sanitise pass is a no-op on the rewrite axis.
        std::env::set_var(
            "JWT_SECRET",
            "test-secret-for-image-proxy-signing-deterministic",
        );
        let input = r#"<img src="https://cdn.example.com/x.png">"#;
        let once = sanitise(input).html;
        let twice = sanitise(&once).html;
        assert_eq!(once, twice, "second sanitise is a no-op");
    }

    #[test]
    fn distinct_trackers_each_listed() {
        let input = r#"
            <img src="https://track.mailchimp.com/a.gif">
            <img src="https://email.sendgrid.net/b.gif">
        "#;
        let out = sanitise(input);
        assert!(out.trackers_stripped.contains(&"Mailchimp".to_string()));
        assert!(out.trackers_stripped.contains(&"SendGrid".to_string()));
        assert_eq!(out.trackers_stripped.len(), 2);
    }

    #[test]
    fn preserves_legitimate_email_structure() {
        // Realistic email body: table-based layout, inline styles,
        // images, links. The sanitiser should keep all of this.
        let input = r#"
            <table cellpadding="0" cellspacing="0" style="width: 100%">
              <tr>
                <td style="padding: 16px">
                  <h1>Order Confirmation</h1>
                  <p>Thanks for your order, <strong>Kyle</strong>!</p>
                  <a href="https://example.com/order/123">View order</a>
                </td>
              </tr>
            </table>
        "#;
        let out = sanitise(input).html;
        assert!(out.contains("Order Confirmation"));
        assert!(out.contains("<table"));
        assert!(out.contains("View order"));
        assert!(out.contains("example.com/order/123"));
    }
}
