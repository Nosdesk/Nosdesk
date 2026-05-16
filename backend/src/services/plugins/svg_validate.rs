//! Plugin icon SVG validator.
//!
//! Plugin icons live in `icon.svg` at the root of the signed zip.
//! Even though the load-bearing browser-side defense is rendering
//! via `<img src=...>` (which sandboxes embedded scripts), the
//! backend refuses to store anything obviously hostile. This is
//! defense-in-depth, not a primary boundary.
//!
//! Refused:
//!   - non-XML bytes
//!   - root element other than `<svg>`
//!   - any `<script>` descendant
//!   - any `<foreignObject>` descendant (HTML smuggling)
//!   - any attribute starting with `on` (event handlers)
//!   - any `href` / `xlink:href` pointing to anything other than
//!     a `data:` URI or a `#fragment` reference
//!
//! Soft cap: 64 KB. Realistic icons are 1-5 KB. Anything bigger is
//! probably a mistake, and SVG content scales independently of
//! file size anyway.

/// Hard size cap on icon bytes accepted by the validator.
pub const MAX_ICON_SIZE: usize = 64 * 1024;

#[derive(Debug)]
pub enum SvgValidationError {
    Empty,
    TooLarge { size: usize },
    NotXml(String),
    NotSvgRoot { found: String },
    DisallowedElement { name: String },
    DisallowedAttribute { name: String, element: String },
    DisallowedHref { value: String },
}

impl std::fmt::Display for SvgValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "icon is empty"),
            Self::TooLarge { size } => write!(f, "icon is {size} bytes; max {MAX_ICON_SIZE}"),
            Self::NotXml(m) => write!(f, "icon is not well-formed XML: {m}"),
            Self::NotSvgRoot { found } => {
                write!(f, "icon root element is {found:?}, expected <svg>")
            }
            Self::DisallowedElement { name } => {
                write!(f, "icon contains disallowed element {name:?}")
            }
            Self::DisallowedAttribute { name, element } => write!(
                f,
                "icon element {element:?} has disallowed attribute {name:?}"
            ),
            Self::DisallowedHref { value } => {
                write!(f, "icon href {value:?} is not a data: URI or fragment")
            }
        }
    }
}

impl std::error::Error for SvgValidationError {}

/// Validate icon bytes. Caller passes the raw `icon.svg` contents
/// from the signed zip; on success the same bytes are stored
/// verbatim and served to clients.
pub fn validate(bytes: &[u8]) -> Result<(), SvgValidationError> {
    if bytes.is_empty() {
        return Err(SvgValidationError::Empty);
    }
    if bytes.len() > MAX_ICON_SIZE {
        return Err(SvgValidationError::TooLarge { size: bytes.len() });
    }

    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut seen_root = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(SvgValidationError::NotXml(e.to_string())),
            Ok(Event::Eof) => break,

            // Empty `<foo .../>` is the same shape as Start for our
            // purposes; both run through the validator.
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let raw_name = e.name();
                let name_bytes = raw_name.as_ref();
                let name_str = std::str::from_utf8(name_bytes).map_err(|err| {
                    SvgValidationError::NotXml(format!("non-utf8 element name: {err}"))
                })?;
                let local = local_name(name_str);

                if !seen_root {
                    if local != "svg" {
                        return Err(SvgValidationError::NotSvgRoot {
                            found: name_str.to_string(),
                        });
                    }
                    seen_root = true;
                }

                if matches!(local, "script" | "foreignObject") {
                    return Err(SvgValidationError::DisallowedElement {
                        name: name_str.to_string(),
                    });
                }

                check_attributes(e, name_str)?;
            }

            // We don't care about End / Text / Comment / DocType
            // shapes; they can't carry script.
            _ => {}
        }
        buf.clear();
    }

    if !seen_root {
        return Err(SvgValidationError::NotSvgRoot {
            found: "(no element)".into(),
        });
    }
    Ok(())
}

fn check_attributes(
    e: &quick_xml::events::BytesStart<'_>,
    element: &str,
) -> Result<(), SvgValidationError> {
    for attr_result in e.attributes().with_checks(false) {
        let attr = attr_result
            .map_err(|err| SvgValidationError::NotXml(format!("attribute parse: {err}")))?;
        let key_bytes = attr.key.as_ref();
        let key_str = std::str::from_utf8(key_bytes)
            .map_err(|err| SvgValidationError::NotXml(format!("non-utf8 attribute name: {err}")))?;
        let key_lower = key_str.to_ascii_lowercase();
        let key_local = local_name(&key_lower);

        if key_local.starts_with("on") {
            return Err(SvgValidationError::DisallowedAttribute {
                name: key_str.to_string(),
                element: element.to_string(),
            });
        }

        if key_local == "href" {
            let raw_value = attr
                .unescape_value()
                .map_err(|err| SvgValidationError::NotXml(format!("attr value: {err}")))?;
            let trimmed = raw_value.trim();
            // Allowlist: empty, fragment, or data: URI. Anything
            // else (http, https, file, javascript, etc.) is refused.
            let allowed =
                trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("data:");
            if !allowed {
                return Err(SvgValidationError::DisallowedHref {
                    value: trimmed.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Local part of an XML name, dropping any namespace prefix. SVG
/// uses `xlink:href` which we want to treat the same as `href`.
fn local_name(qualified: &str) -> &str {
    qualified
        .rsplit_once(':')
        .map(|(_, rhs)| rhs)
        .unwrap_or(qualified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_minimal_svg() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16"/></svg>"#;
        validate(svg).unwrap();
    }

    #[test]
    fn rejects_empty() {
        assert!(matches!(validate(b""), Err(SvgValidationError::Empty)));
    }

    #[test]
    fn rejects_oversized() {
        let big = vec![b'x'; MAX_ICON_SIZE + 1];
        assert!(matches!(
            validate(&big),
            Err(SvgValidationError::TooLarge { .. })
        ));
    }

    #[test]
    fn rejects_non_xml() {
        let bytes = b"not actually xml at all";
        assert!(matches!(
            validate(bytes),
            Err(SvgValidationError::NotXml(_) | SvgValidationError::NotSvgRoot { .. })
        ));
    }

    #[test]
    fn rejects_non_svg_root() {
        let html = br#"<html><body>nope</body></html>"#;
        match validate(html) {
            Err(SvgValidationError::NotSvgRoot { .. }) => {}
            other => panic!("expected NotSvgRoot, got {other:?}"),
        }
    }

    #[test]
    fn rejects_script() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
        match validate(svg) {
            Err(SvgValidationError::DisallowedElement { name }) if name == "script" => {}
            other => panic!("expected DisallowedElement, got {other:?}"),
        }
    }

    #[test]
    fn rejects_foreign_object() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><foreignObject><div/></foreignObject></svg>"#;
        match validate(svg) {
            Err(SvgValidationError::DisallowedElement { name }) if name == "foreignObject" => {}
            other => panic!("expected DisallowedElement, got {other:?}"),
        }
    }

    #[test]
    fn rejects_event_handler() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"></svg>"#;
        match validate(svg) {
            Err(SvgValidationError::DisallowedAttribute { name, .. }) if name == "onload" => {}
            other => panic!("expected DisallowedAttribute, got {other:?}"),
        }
    }

    #[test]
    fn rejects_external_href() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><image href="http://attacker.test/track.png"/></svg>"#;
        match validate(svg) {
            Err(SvgValidationError::DisallowedHref { .. }) => {}
            other => panic!("expected DisallowedHref, got {other:?}"),
        }
    }

    #[test]
    fn allows_data_uri_image() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><image href="data:image/png;base64,AAA"/></svg>"#;
        validate(svg).unwrap();
    }

    #[test]
    fn allows_fragment_href() {
        // Doubled-pound raw byte string because the content's
        // `"#icon...` would close `br#"..."#` at the wrong spot.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg"><use href="#icon-shape"/></svg>"##;
        validate(svg).unwrap();
    }

    #[test]
    fn rejects_xlink_external() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><use xlink:href="https://attacker.test/icon.svg#x"/></svg>"##;
        match validate(svg) {
            Err(SvgValidationError::DisallowedHref { .. }) => {}
            other => panic!("expected DisallowedHref, got {other:?}"),
        }
    }
}
