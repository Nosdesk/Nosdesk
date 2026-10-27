//! The images an imported page shows: where each comes from, whether it
//! can be stored, and the name it is stored under.
//!
//! A converted page's image sources are one of: a placeholder for a file in
//! the archive, a `data:` URI written into the page, a web URL, or an image
//! already stored in this workspace (a Nosdesk export of it). Every one is
//! stored with the page that shows it at the commit, through the same
//! checks the editor's image upload makes (image types only, 10 MB, SVG
//! refused). A `data:` source never reaches a written document.

use base64::Engine as _;

use super::{ImageKind, ImageProblem};
use crate::services::docs_markdown::ir::{Block, Document, Image, Inline, ListItem};
use crate::services::docs_markdown::links::Placeholder;
use crate::utils::file_validation::{FileValidationError, FileValidator};

/// The collab image cap.
pub const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;

/// Where the editor's images are served from.
const SERVED_PREFIX: &str = "/api/files/collab/doc/";

/// Call `f` with every image in the document, in order.
pub fn for_each_image(document: &Document, f: &mut dyn FnMut(&Image)) {
    fn blocks(list: &[Block], f: &mut dyn FnMut(&Image)) {
        for block in list {
            match block {
                Block::Paragraph(inlines)
                | Block::Heading {
                    content: inlines, ..
                } => {
                    for inline in inlines {
                        if let Inline::Image(image) = inline {
                            f(image);
                        }
                    }
                }
                Block::Blockquote(inner) => blocks(inner, f),
                Block::BulletList(items) | Block::OrderedList { items, .. } => {
                    for item in items {
                        blocks(&item.blocks, f);
                    }
                }
                Block::Table(table) => {
                    for cell in table.rows.iter().flatten() {
                        blocks(&cell.blocks, f);
                    }
                }
                Block::CodeBlock { .. }
                | Block::HorizontalRule
                | Block::EmbeddedDocument { .. } => {}
            }
        }
    }
    blocks(&document.blocks, f);
}

/// Call `f` with every image in the document, in order, to change it.
pub fn for_each_image_mut(document: &mut Document, f: &mut dyn FnMut(&mut Image)) {
    fn blocks(list: &mut [Block], f: &mut dyn FnMut(&mut Image)) {
        for block in list {
            match block {
                Block::Paragraph(inlines)
                | Block::Heading {
                    content: inlines, ..
                } => {
                    for inline in inlines.iter_mut() {
                        if let Inline::Image(image) = inline {
                            f(image);
                        }
                    }
                }
                Block::Blockquote(inner) => blocks(inner, f),
                Block::BulletList(items) | Block::OrderedList { items, .. } => items_mut(items, f),
                Block::Table(table) => {
                    for cell in table.rows.iter_mut().flatten() {
                        blocks(&mut cell.blocks, f);
                    }
                }
                Block::CodeBlock { .. }
                | Block::HorizontalRule
                | Block::EmbeddedDocument { .. } => {}
            }
        }
    }
    fn items_mut(items: &mut [ListItem], f: &mut dyn FnMut(&mut Image)) {
        for item in items {
            blocks(&mut item.blocks, f);
        }
    }
    blocks(&mut document.blocks, f);
}

/// Every embedded page's `documentUuid`, in order.
pub fn embedded_uuids(document: &Document) -> Vec<String> {
    fn blocks(list: &[Block], out: &mut Vec<String>) {
        for block in list {
            match block {
                Block::EmbeddedDocument { document_uuid, .. } => out.push(document_uuid.clone()),
                Block::Blockquote(inner) => blocks(inner, out),
                Block::BulletList(items) | Block::OrderedList { items, .. } => {
                    for item in items {
                        blocks(&item.blocks, out);
                    }
                }
                Block::Table(table) => {
                    for cell in table.rows.iter().flatten() {
                        blocks(&cell.blocks, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    blocks(&document.blocks, &mut out);
    out
}

/// A web image's source: http or https.
pub fn is_web(src: &str) -> bool {
    let lower = src.get(..8).unwrap_or(src).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

pub fn is_data(src: &str) -> bool {
    src.get(..5)
        .is_some_and(|s| s.eq_ignore_ascii_case("data:"))
}

/// The storage key of an image this workspace serves, from its source.
pub fn workspace_image_key(src: &str) -> Option<String> {
    let rest = src.strip_prefix(SERVED_PREFIX)?;
    let rest = rest.split(['?', '#']).next()?;
    let (page, file) = rest.split_once('/')?;
    let page = uuid::Uuid::parse_str(page).ok()?;
    if file.is_empty() || file.contains('/') || file.contains("..") {
        return None;
    }
    Some(format!("collab/doc/{page}/{file}"))
}

/// The page a `collab/doc/{page}/...` key belongs to.
pub fn page_of_workspace_key(key: &str) -> Option<uuid::Uuid> {
    let rest = key.strip_prefix("collab/doc/")?;
    uuid::Uuid::parse_str(rest.split('/').next()?).ok()
}

/// The source an image stored under `key` is served at.
pub fn served_src(key: &str) -> String {
    format!("/api/files/{}", key.trim_start_matches('/'))
}

/// What an image source is, for one that can be stored: `None` for a
/// source the import leaves as it is (a kept web link).
pub fn kind_of(src: &str) -> Option<ImageKind> {
    if let Some(Placeholder::Asset(_)) = Placeholder::parse(src) {
        return Some(ImageKind::Archive);
    }
    if is_data(src) {
        return Some(ImageKind::Data);
    }
    if is_web(src) {
        return Some(ImageKind::Web);
    }
    if workspace_image_key(src).is_some() {
        return Some(ImageKind::Workspace);
    }
    None
}

/// How a source is listed: the archive path, the URL, `data:image/png`.
pub fn source_label(src: &str) -> String {
    if let Some(Placeholder::Asset(key)) = Placeholder::parse(src) {
        return key;
    }
    if is_data(src) {
        let head = src.split([';', ',']).next().unwrap_or("data:");
        return head.chars().take(40).collect();
    }
    src.chars().take(2048).collect()
}

/// The file name an image is stored under, from its source.
pub fn name_of(src: &str, content_type: Option<&str>) -> String {
    let raw = if let Some(Placeholder::Asset(key)) = Placeholder::parse(src) {
        key.rsplit('/').next().unwrap_or(&key).to_string()
    } else if is_data(src) {
        String::new()
    } else {
        let path = src
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(src);
        let last = path.rsplit('/').next().unwrap_or("");
        crate::services::docs_markdown::links::percent_decode(last)
    };
    let sanitized = FileValidator::sanitize_filename(&raw).ok();
    let extension = content_type.and_then(extension_for);
    match (sanitized, extension) {
        (Some(name), Some(ext)) if !has_extension(&name) => format!("{name}.{ext}"),
        (Some(name), _) => name,
        (None, ext) => format!("image.{}", ext.unwrap_or("png")),
    }
}

fn has_extension(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && !ext.is_empty() && ext.len() <= 5)
}

/// The extension an image type is stored with.
pub fn extension_for(content_type: &str) -> Option<&'static str> {
    Some(match content_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/avif" => "avif",
        "image/tiff" => "tiff",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        _ => return None,
    })
}

/// Decode a `data:image/...;base64,` source, refusing one larger than the
/// image cap before decoding it.
pub fn decode_data(src: &str) -> Result<Vec<u8>, ImageProblem> {
    let (_, payload) = src.split_once(',').ok_or(ImageProblem::Unreadable)?;
    let payload = payload.trim();
    // Base64 is 4 characters for every 3 bytes.
    if (payload.len() as u64) / 4 * 3 > MAX_IMAGE_BYTES + 3 {
        return Err(ImageProblem::TooLarge);
    }
    let cleaned: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(cleaned.as_bytes()))
        .map_err(|_| ImageProblem::Unreadable)?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(ImageProblem::TooLarge);
    }
    Ok(bytes)
}

/// The image's type when the editor's upload would take it: sniffed from
/// its bytes, image types only, SVG refused.
pub fn validate(bytes: &[u8], name: &str) -> Result<String, ImageProblem> {
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(ImageProblem::TooLarge);
    }
    match FileValidator::validate_file(bytes, Some(name)) {
        Ok(mime) if mime.starts_with("image/") => Ok(mime),
        Ok(_) => Err(ImageProblem::NotAnImage),
        Err(FileValidationError::BlockedMimeType { detected }) if detected.contains("svg") => {
            Err(ImageProblem::Svg)
        }
        Err(FileValidationError::BlockedExtension { extension })
            if extension.starts_with("svg") =>
        {
            Err(ImageProblem::Svg)
        }
        Err(_) => Err(ImageProblem::NotAnImage),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The smallest valid PNG.
    pub(crate) const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn sources_are_told_apart() {
        let asset = Placeholder::Asset("img/a b.png".into()).to_href();
        assert_eq!(kind_of(&asset), Some(ImageKind::Archive));
        assert_eq!(kind_of("data:image/png;base64,AAAA"), Some(ImageKind::Data));
        assert_eq!(kind_of("HTTPS://example.com/x.png"), Some(ImageKind::Web));
        let stored = format!("/api/files/collab/doc/{}/0199_a.png", uuid::Uuid::now_v7());
        assert_eq!(kind_of(&stored), Some(ImageKind::Workspace));
        assert_eq!(kind_of("/api/files/collab/doc/not-a-uuid/a.png"), None);
        assert_eq!(source_label(&asset), "img/a b.png");
        assert_eq!(source_label("data:image/png;base64,AAAA"), "data:image/png");
    }

    #[test]
    fn stored_names_are_safe_and_typed() {
        let asset = Placeholder::Asset("img/vpn diagram.png".into()).to_href();
        assert_eq!(name_of(&asset, Some("image/png")), "vpndiagram.png");
        assert_eq!(
            name_of("https://example.com/a/logo?size=2", Some("image/jpeg")),
            "logo.jpg"
        );
        assert_eq!(
            name_of("data:image/gif;base64,R0lG", Some("image/gif")),
            "image.gif"
        );
        assert_eq!(name_of("https://example.com/", None), "image.png");
    }

    #[test]
    fn data_images_decode_under_the_cap() {
        let src = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(PNG)
        );
        let bytes = decode_data(&src).unwrap();
        assert_eq!(bytes, PNG);
        assert_eq!(validate(&bytes, "image.png").unwrap(), "image/png");
        assert_eq!(
            decode_data("data:image/png;base64,***"),
            Err(ImageProblem::Unreadable)
        );
        let huge = format!("data:image/png;base64,{}", "A".repeat(15 * 1024 * 1024));
        assert_eq!(decode_data(&huge), Err(ImageProblem::TooLarge));
    }

    #[test]
    fn svg_and_other_files_are_refused() {
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>";
        assert_eq!(validate(svg, "a.png"), Err(ImageProblem::Svg));
        assert_eq!(validate(PNG, "a.svg"), Err(ImageProblem::Svg));
        assert_eq!(
            validate(b"%PDF-1.4 hello", "a.pdf"),
            Err(ImageProblem::NotAnImage)
        );
        assert_eq!(
            validate(b"plain text", "a.png"),
            Err(ImageProblem::NotAnImage)
        );
    }

    #[test]
    fn workspace_keys_stay_in_the_collab_folder() {
        let page = uuid::Uuid::now_v7();
        let src = format!("/api/files/collab/doc/{page}/0199_a.png");
        let key = workspace_image_key(&src).unwrap();
        assert_eq!(key, format!("collab/doc/{page}/0199_a.png"));
        assert_eq!(page_of_workspace_key(&key), Some(page));
        assert_eq!(served_src(&key), src);
        assert_eq!(
            workspace_image_key(&format!("/api/files/collab/doc/{page}/../x")),
            None
        );
    }
}
