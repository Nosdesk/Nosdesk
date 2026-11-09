//! The seam between a documentation source and the planner.
//!
//! A [`DocsSource`] reads what one tool exported into a [`SourceTree`]:
//! its pages with their titles and bodies, siblings in the source's order,
//! and what it says about each page (status, icon, slug, other names links
//! may use). It reads and never converts until asked. The planner in
//! [`super::tree`] takes the tree from there and owns everything sources
//! share: leaving out unticked pages, status and collection modes, long
//! titles, duplicate keys, the link index, notes and link reports.
//!
//! Every profile today is one source, [`MarkdownFamily`]: the Markdown
//! layouts of Nosdesk, Notion, Obsidian, GitBook, MkDocs, Docusaurus,
//! Outline and plain folders.
//!
//! [`MarkdownFamily`]: super::markdown_family::MarkdownFamily

use super::ir::Document;
use super::profiles::ProfileKind;
use super::tree::{CollectionMode, LeftOut, PageKind, PageStatus, PlannedCollection};
use super::{Notes, Resolver};
use crate::services::imports::refusal::Refused;
use crate::services::uploaded_archive::SafeArchive;

/// What a source reads from.
pub enum SourceInput<'a> {
    /// An uploaded archive.
    Archive {
        archive: &'a mut SafeArchive,
        /// Entry keys the leave-out patterns kept.
        files: &'a [String],
    },
}

/// One tool's export, read into a tree.
pub trait DocsSource {
    fn kind(&self) -> ProfileKind;

    /// The tree as the source describes it. Reads, never converts.
    fn read(&mut self, input: &mut SourceInput<'_>) -> Result<SourceTree, Refused>;

    /// One page's body as an IR document, not yet normalised: links and
    /// images go through `resolver`, degrades are counted in `notes`.
    /// Called once per planned page whose body isn't [`Body::Empty`] and
    /// that has no [`SourcePage::error`]; `page.title` is the planned title.
    fn convert(&self, page: &SourcePage, resolver: &dyn Resolver, notes: &mut Notes) -> Document;
}

/// The pages a source holds, before the planner applies the admin's
/// options.
#[derive(Debug)]
pub struct SourceTree {
    /// The folder the source sits in inside the archive, if any. Links
    /// resolve against it.
    pub root: String,
    /// What the source calls itself (an MkDocs `site_name`), for the
    /// default collection name.
    pub site_name: Option<String>,
    /// The collection mode when the admin picks none.
    pub default_collections: CollectionMode,
    /// The source's own collections, in their order, when it has them (even
    /// none): `as_source` follows them, and [`SourcePage::collection`]
    /// points into them.
    pub collections: Option<Vec<PlannedCollection>>,
    pub pages: Vec<SourcePage>,
    /// The top-level pages in order.
    pub top: Vec<usize>,
    /// Archive files a page may link to or show, by key.
    pub files: Vec<String>,
    /// Files the source leaves out, and why.
    pub left_out: Vec<LeftOut>,
    /// The workspace a Nosdesk export came from, for keeping its mentions,
    /// ticket links and embeds when it is this one.
    pub workspace_uuid: Option<String>,
    /// Destinations in the source's own scheme (BookStack's `bsexport://`).
    pub refs: SourceRefs,
}

/// The destinations a source writes in a scheme of its own, and what each
/// names. Only a source fills these: the scheme means nothing to any other
/// source's links, which drop it as unknown.
#[derive(Debug, Clone, Default)]
pub struct SourceRefs {
    /// The schemes the source writes (`bsexport`). A destination in one of
    /// them that names nothing is reported as unresolved.
    pub schemes: Vec<&'static str>,
    /// Each destination, without its fragment, as the source wrote it into
    /// the page bodies.
    pub targets: std::collections::HashMap<String, SourceRef>,
}

/// What one of a source's own destinations names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    /// A page, by its index in [`SourceTree::pages`].
    Page(usize),
    /// One of the source's collections, by its index in
    /// [`SourceTree::collections`].
    Collection(usize),
    /// An archive file shown as an image, by key.
    Image(String),
    /// A file the import leaves out, by the name Review lists it under.
    Attachment(String),
    /// A web address.
    Web(String),
}

/// One page as the source describes it.
#[derive(Debug, Clone, Default)]
pub struct SourcePage {
    /// What a later import of the same export finds this page by: recorded
    /// with each page the commit writes. For a Markdown family it is the
    /// archive path of the page's file, or of the folder a page without one
    /// stands for, as every import before this seam recorded it.
    pub key: String,
    /// The page's id in the tool it came from, when it has one, for the
    /// conversion report.
    pub source_id: Option<String>,
    /// The archive file the page is read from.
    pub file: Option<String>,
    /// The archive folder the page stands for.
    pub folder: Option<String>,
    /// Archive entries the page's content comes from; leaving the page out
    /// leaves them out.
    pub entries: Vec<String>,
    /// The title in full; the planner shortens a long one.
    pub title: String,
    /// Child pages in order.
    pub children: Vec<usize>,
    /// A folder or a heading group: it may become a collection.
    pub group: bool,
    /// The source collection a top-level page goes in, under `as_source`.
    pub collection: Option<usize>,
    /// What the page stands for in the source's tree. A
    /// [`PageKind::DatabaseRow`] whose database ends up elsewhere is planned
    /// as a page.
    pub kind: PageKind,
    pub body: Body,
    /// Why the page's content couldn't be read; it is imported empty.
    pub error: Option<String>,
    /// The status the source gives the page, honoured when the admin
    /// keeps the source's statuses.
    pub status: Option<PageStatus>,
    pub icon: Option<String>,
    /// A slug to keep when it is free.
    pub slug: Option<String>,
    /// A Nosdesk uuid, for spotting a page imported before.
    pub uuid: Option<String>,
    /// Other names links may reach the page by.
    pub names: LinkNames,
    /// Metadata keys the import used, and those it didn't.
    pub metadata_used: Vec<String>,
    pub metadata_not_imported: Vec<String>,
    /// Degrades found while reading, before conversion.
    pub notes: Notes,
}

impl SourcePage {
    /// Whether the page brings content of its own, rather than only
    /// standing for its children.
    pub fn has_content(&self) -> bool {
        !matches!(self.body, Body::Empty) || self.error.is_some()
    }
}

/// Aliases, slugs and uuids a link may name a page by.
#[derive(Debug, Clone, Default)]
pub struct LinkNames {
    pub aliases: Vec<String>,
    pub slugs: Vec<String>,
    pub uuids: Vec<String>,
}

/// A page's content as the source holds it.
#[derive(Debug, Clone, Default)]
pub enum Body {
    /// Markdown, its links relative to the page's file.
    Markdown(String),
    /// HTML the source reads with its dialect
    /// ([`read_document`](super::html::read_document)); links resolve as
    /// Markdown's do.
    Html(String),
    /// A CSV shown as a table (a Notion database); `None` when it couldn't
    /// be read.
    Csv(Option<String>),
    /// No content of its own.
    #[default]
    Empty,
}

impl Body {
    /// Whether the body holds links and images the import resolves.
    pub fn has_links(&self) -> bool {
        matches!(self, Body::Markdown(_) | Body::Html(_))
    }
}
