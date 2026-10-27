//! A page as Review previews it: the converted document as ProseMirror
//! JSON, read back from the same Yjs encoding the commit writes, so the
//! client builds exactly what the page will hold.
//!
//! Images show as a named placeholder (`nosdesk-import:preview-image/{n}`
//! with the file's name, size and any problem in `images`), so no route
//! serves an image that isn't stored yet. Links and embedded pages inside
//! the import keep their `nosdesk-import:page/{index}` destination; the
//! commit swaps those for the pages it creates.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::images;
use super::{ImageKind, ImageProblem, PageImage};
use crate::services::docs_markdown::encode::{self, decode};
use crate::services::docs_markdown::ir::Document;

/// The source a preview's image placeholder carries.
pub const IMAGE_PREFIX: &str = "nosdesk-import:preview-image/";

/// One image placeholder's file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewImage {
    pub name: String,
    pub size: Option<u64>,
    pub kind: ImageKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<ImageProblem>,
}

/// A page's preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preview {
    pub index: usize,
    pub title: String,
    /// ProseMirror JSON (`{"type": "doc", ...}`).
    pub doc: serde_json::Value,
    /// Each image placeholder's file, by its source.
    pub images: BTreeMap<String, PreviewImage>,
}

/// The preview of `document`, whose images the check described in `found`
/// (by [`images::source_label`]).
pub fn preview(index: usize, title: &str, mut document: Document, found: &[PageImage]) -> Preview {
    let by_source: BTreeMap<&str, &PageImage> =
        found.iter().map(|i| (i.source.as_str(), i)).collect();
    let mut placeholders = BTreeMap::new();
    images::for_each_image_mut(&mut document, &mut |image| {
        let label = images::source_label(&image.src);
        let info = match by_source.get(label.as_str()) {
            Some(found) => PreviewImage {
                name: found.name.clone(),
                size: found.size,
                kind: found.kind,
                problem: found.problem,
            },
            None => PreviewImage {
                name: images::name_of(&image.src, None),
                size: None,
                kind: images::kind_of(&image.src).unwrap_or(ImageKind::Web),
                problem: None,
            },
        };
        let src = format!("{IMAGE_PREFIX}{}", placeholders.len());
        placeholders.insert(src.clone(), info);
        image.src = src;
    });
    let update = encode::encode(&document);
    let doc = decode::try_to_json(&update)
        .unwrap_or_else(|| serde_json::json!({ "type": "doc", "content": [] }));
    Preview {
        index,
        title: title.to_string(),
        doc,
        images: placeholders,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::docs_markdown::ir::{Block, Image, Inline};
    use crate::services::docs_markdown::links::Placeholder;

    #[test]
    fn images_become_named_placeholders_and_no_data_uri_survives() {
        let asset = Placeholder::Asset("img/net.png".into()).to_href();
        let document = Document {
            blocks: vec![Block::Paragraph(vec![
                Inline::text("See "),
                Inline::Image(Image {
                    src: asset,
                    alt: Some("network".into()),
                    ..Image::default()
                }),
                Inline::Image(Image {
                    src: "data:image/png;base64,iVBORw0KGgo=".into(),
                    ..Image::default()
                }),
            ])],
        };
        let found = vec![PageImage {
            source: "img/net.png".into(),
            kind: ImageKind::Archive,
            name: "net.png".into(),
            size: Some(240_000),
            problem: None,
        }];
        let p = preview(2, "Network", document, &found);
        let text = p.doc.to_string();
        assert!(!text.contains("data:"), "{text}");
        assert!(!text.contains("nosdesk-import:asset"), "{text}");
        assert_eq!(p.doc["content"][0]["type"], "paragraph");
        let first = &p.doc["content"][0]["content"][1];
        assert_eq!(first["type"], "image");
        let src = first["attrs"]["src"].as_str().unwrap();
        assert_eq!(p.images[src].name, "net.png");
        assert_eq!(p.images[src].size, Some(240_000));
        assert_eq!(p.images.len(), 2);
    }
}
