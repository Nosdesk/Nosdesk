//! Markdown to Nosdesk documents.
//!
//! Three layers: [`parse`] reads Markdown (pulldown-cmark) into the
//! [`ir`] tree, [`normalise`] makes that tree valid under the editor schema,
//! and [`encode`] writes it to Yjs exactly as y-prosemirror does.
//!
//! Validity is not optional. y-prosemirror deletes any Yjs element it cannot
//! build into a ProseMirror node, from the shared document, for everyone, on
//! the first client that opens the page. So everything this module writes
//! goes through the normaliser, and the golden fixtures under
//! `tests/fixtures/docs_markdown/` are checked against the real schema by
//! `frontend/src/components/editor/docsMarkdownFixtures.spec.ts`.
//!
//! What the conversion loses is never silent: [`Converted::notes`] counts each
//! degrade by [`NoteCode`] so an import can show them before anything is
//! written.

pub mod encode;
pub mod ir;
pub mod normalise;
pub mod parse;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Serialize;

pub use encode::encode;
pub use normalise::{DefaultResolver, Resolver};

/// One kind of change the conversion made to the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteCode {
    /// A table became a `markdown` code block holding a pipe table.
    TablesAsText,
    /// A task list became a bullet list with "☐ " and "☑ " items.
    ChecklistsAsBullets,
    /// Strikethrough, underline, highlight, superscript or subscript
    /// formatting was removed; the text is kept.
    StrikethroughRemoved,
    /// A callout became a quote with its label in bold.
    CalloutsAsQuotes,
    /// A formula became inline code or a `latex` code block.
    FormulasAsCode,
    /// A link's destination is not one a page can hold; its text is kept.
    LinksDropped,
    /// An image's source is not one a page can show; its alt text is kept.
    ImagesDropped,
    /// Embedded HTML (scripts, styles, frames, comments) was removed.
    HtmlRemoved,
    /// Other HTML was reduced to its text.
    HtmlAsText,
    /// Nesting deeper than the editor allows was flattened.
    NestingFlattened,
}

/// Degrade counts by code. Ordered, so reports and tests are stable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Notes(BTreeMap<NoteCode, u32>);

impl Notes {
    pub fn add(&mut self, code: NoteCode) {
        *self.0.entry(code).or_insert(0) += 1;
    }

    pub fn count(&self, code: NoteCode) -> u32 {
        self.0.get(&code).copied().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (NoteCode, u32)> + '_ {
        self.0.iter().map(|(code, count)| (*code, *count))
    }

    pub fn merge(&mut self, other: &Notes) {
        for (code, count) in other.iter() {
            *self.0.entry(code).or_insert(0) += count;
        }
    }
}

/// The syntax that delimited a front matter block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontMatterKind {
    /// `---` delimited YAML.
    Yaml,
    /// `+++` delimited (TOML style).
    Pluses,
}

/// A front matter block, unparsed. It is page metadata, never body content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontMatter {
    pub kind: FrontMatterKind,
    pub raw: String,
}

/// A converted page: a schema-valid document and what changed on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converted {
    pub document: ir::Document,
    pub notes: Notes,
    pub front_matter: Option<FrontMatter>,
}

/// Parse and normalise one Markdown page, resolving links and images with
/// `resolver`.
pub fn convert_with(markdown: &str, resolver: &dyn Resolver) -> Converted {
    let parsed = parse::parse(markdown);
    let mut notes = parsed.notes;
    let document = normalise::normalise(parsed.document, resolver, &mut notes);
    Converted {
        document,
        notes,
        front_matter: parsed.front_matter,
    }
}

/// Parse and normalise one Markdown page with no archive around it: web,
/// mail and phone links are kept, anything else becomes text.
pub fn convert(markdown: &str) -> Converted {
    convert_with(markdown, &DefaultResolver)
}

/// Markdown to a y-prosemirror Yjs update (v1), plus the degrade notes.
pub fn markdown_to_yjs(markdown: &str) -> (Vec<u8>, Notes) {
    let converted = convert(markdown);
    (encode(&converted.document), converted.notes)
}
