//! `data.json` as BookStack's Portable ZIP writes it, typed.
//!
//! Every field the format calls optional is optional here, and `null` reads
//! as absent. Unknown fields are ignored: the format adds properties in any
//! release. What the import doesn't take (a book's cover and tags, image
//! names and types) isn't read; page and chapter tags are only counted, so
//! their contents are skipped unread.

use serde::de::IgnoredAny;
use serde::Deserialize;

/// The root of `data.json`. Exactly one of `book`, `chapter` and `page` is
/// set in an export this importer reads.
#[derive(Debug, Default, Deserialize)]
pub struct DataJson {
    #[serde(default)]
    pub instance: Option<Instance>,
    #[serde(default)]
    pub book: Option<Book>,
    #[serde(default)]
    pub chapter: Option<Chapter>,
    #[serde(default)]
    pub page: Option<Page>,
}

/// The instance an export came from.
#[derive(Debug, Default, Deserialize)]
pub struct Instance {
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Book {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description_html: Option<String>,
    #[serde(default)]
    pub chapters: Option<Vec<Chapter>>,
    /// The book's direct pages, not those in its chapters.
    #[serde(default)]
    pub pages: Option<Vec<Page>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Chapter {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description_html: Option<String>,
    /// Order within the book, low to high.
    #[serde(default)]
    pub priority: Option<f64>,
    #[serde(default)]
    pub pages: Option<Vec<Page>>,
    #[serde(default)]
    pub tags: Option<Vec<IgnoredAny>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Page {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    /// The page as BookStack renders it, includes expanded. Always written,
    /// a Markdown page's too.
    #[serde(default)]
    pub html: Option<String>,
    /// What a Markdown page's author wrote; absent for a WYSIWYG page.
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub priority: Option<f64>,
    #[serde(default)]
    pub attachments: Option<Vec<Attachment>>,
    #[serde(default)]
    pub images: Option<Vec<Image>>,
    #[serde(default)]
    pub tags: Option<Vec<IgnoredAny>>,
}

/// An image a page shows. Its references can come from any page in the
/// export, not only this one.
#[derive(Debug, Default, Deserialize)]
pub struct Image {
    #[serde(default)]
    pub id: Option<i64>,
    /// A file under `files/`.
    #[serde(default)]
    pub file: Option<String>,
}

/// A file uploaded to a page (`file`, under `files/`) or a link (`link`).
#[derive(Debug, Default, Deserialize)]
pub struct Attachment {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub file: Option<String>,
}

/// What the top-level keys of a `data.json` say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// One book, chapter or page.
    Export,
    /// A BookStack export holding something else, or more than one thing
    /// (a future `books`).
    Unsupported,
    /// Not BookStack's.
    Other,
}

/// The shape of `data.json` from its top-level keys alone: values are
/// skipped, not kept.
pub fn shape(bytes: &[u8]) -> Option<Shape> {
    let keys: std::collections::BTreeMap<String, IgnoredAny> =
        serde_json::from_slice(bytes).ok()?;
    let content = ["book", "chapter", "page"]
        .iter()
        .filter(|k| keys.contains_key(**k))
        .count();
    Some(match content {
        1 => Shape::Export,
        0 if keys.contains_key("instance") || keys.contains_key("exported_at") => {
            Shape::Unsupported
        }
        0 => Shape::Other,
        _ => Shape::Unsupported,
    })
}
