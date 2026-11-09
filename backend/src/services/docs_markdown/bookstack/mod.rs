//! BookStack's Portable ZIP (BookStack 24.12 and later) as a documentation
//! source.
//!
//! An export is `data.json` beside a `files/` folder. `data.json` holds one
//! book, chapter or page; images and attachments name a file under
//! `files/` by a random name BookStack gave it. Content refers to anything
//! else in the export as `[[bsexport:page|chapter|book|image|attachment:ID]]`
//! in place of a URL; links to anything outside it stay absolute URLs to
//! the source instance. A zip of several exports imports each as its own
//! collection.
//!
//! - A book is a collection; its direct pages and chapters are its top
//!   level, merged and ordered by `priority` as BookStack orders them.
//!   A chapter is a page (its description its body) with its pages under
//!   it. A chapter or page export is a collection named after it.
//! - A page's body is its `markdown` when that isn't empty (what the author
//!   wrote), else its `html`. References are rewritten to
//!   `bsexport://{export}/{type}/{id}` before either is read; that scheme
//!   resolves only through the destinations this source registers
//!   ([`SourceRefs`]), so no other source's text can reach them.
//! - Page includes (`{{@12#bkmrk-x}}`) are already expanded in `html`;
//!   in `markdown` each becomes a link "Included from {title}" when the page
//!   is in the import, else stays as text.
//! - Images come from `files/`, wherever in the export the image is listed.
//!   File attachments are left out and listed by page; link attachments
//!   are kept as an "Attachments" list of links at the page's end.
//! - Tags are not imported; pages with them are counted.

mod data;
mod dialect;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use super::html;
use super::ir::{self, Block, Document, Inline, Link, ListItem, Marks};
use super::parse::{self, ParseOptions};
use super::profiles::ProfileKind;
use super::source::{Body, DocsSource, SourceInput, SourcePage, SourceRef, SourceRefs, SourceTree};
use super::tree::{
    fit_title, CollectionMode, PageKind, PlannedCollection, MAX_MARKDOWN_BYTES, MAX_PAGES,
};
use super::{DefaultResolver, NoteCode, Notes, Resolver};
use crate::services::imports::refusal::Refused;
use crate::services::uploaded_archive::{ArchiveError, SafeArchive};

pub use data::Shape;
pub use dialect::BookstackDialect;

/// The scheme references are rewritten to.
pub const SCHEME: &str = "bsexport";
/// The file every export holds.
pub const DATA_JSON: &str = "data.json";
/// Largest `data.json` read: a book's whole content sits in it.
pub const MAX_DATA_JSON_BYTES: u64 = 64 * 1024 * 1024;
/// Largest total of every `data.json` in a zip of exports.
pub const MAX_DATA_JSON_TOTAL: u64 = 128 * 1024 * 1024;
/// Exports one zip may bundle.
pub const MAX_EXPORTS: usize = 50;
/// Longest collection description kept.
const MAX_DESCRIPTION_CHARS: usize = 2_000;
/// Longest instance id kept in a page's key.
const MAX_INSTANCE_CHARS: usize = 64;
/// The heading of the link attachments list.
const ATTACHMENTS_HEADING: &str = "Attachments";

pub fn refused_unreadable() -> Refused {
    Refused::new(
        "docs_import_bookstack_unreadable",
        "This BookStack export's data.json can't be read.",
    )
}

pub fn refused_too_large() -> Refused {
    Refused::new(
        "docs_import_bookstack_too_large",
        "This BookStack export is larger than we can read at once. Export one chapter at a time.",
    )
    .arg("max", MAX_DATA_JSON_BYTES)
}

pub fn refused_unsupported() -> Refused {
    Refused::new(
        "docs_import_bookstack_unsupported",
        "This BookStack export holds more than this version can read. Export one book at a time.",
    )
}

/// One export in the archive.
struct Export {
    /// The folder its `data.json` sits in, or empty.
    root: String,
    data: data::DataJson,
}

/// The BookStack exports an archive holds, each `data.json` read once.
pub struct Exports(Vec<Export>);

impl Exports {
    /// The exports among `files`: each `data.json` at the root, in a
    /// wrapper folder or in a bundled part's folder, that holds a book,
    /// chapter or page. `None` when there is none.
    ///
    /// A `data.json` beside nothing but `files/` is BookStack's whatever it
    /// holds: one that can't be read, or that is over
    /// [`MAX_DATA_JSON_BYTES`], refuses the archive. Elsewhere such a file
    /// isn't an export and is passed over.
    pub fn find(archive: &mut SafeArchive, files: &[String]) -> Result<Option<Self>, Refused> {
        let mut exports = Vec::new();
        let mut total: u64 = 0;
        let candidates: Vec<String> = files
            .iter()
            .filter(|key| is_data_json(key))
            .cloned()
            .collect();
        for key in candidates {
            let root = super::links::parent(&key).to_string();
            let layout = is_export_layout(files, &key, &root);
            if archive.set_entry_cap(&key, MAX_DATA_JSON_BYTES).is_err() {
                continue;
            }
            let bytes = match archive.read(&key, MAX_DATA_JSON_BYTES) {
                Ok(bytes) => bytes,
                Err(ArchiveError::EntryTooLarge { .. }) if layout => {
                    return Err(refused_too_large())
                }
                Err(_) if layout => return Err(refused_unreadable()),
                Err(_) => continue,
            };
            total += bytes.len() as u64;
            if total > MAX_DATA_JSON_TOTAL {
                return Err(refused_too_large());
            }
            match data::shape(&bytes) {
                Some(Shape::Export) => {}
                Some(Shape::Unsupported) => return Err(refused_unsupported()),
                None if layout => return Err(refused_unreadable()),
                Some(Shape::Other) | None => continue,
            }
            let data: data::DataJson =
                serde_json::from_slice(&bytes).map_err(|_| refused_unreadable())?;
            exports.push(Export { root, data });
            if exports.len() > MAX_EXPORTS {
                return Err(refused_unsupported());
            }
        }
        Ok((!exports.is_empty()).then_some(Exports(exports)))
    }
}

/// `data.json` at the root, or one or two folders down (a wrapper folder,
/// a bundled part, both).
fn is_data_json(key: &str) -> bool {
    let segments = key.split('/').count();
    segments <= 3 && key.rsplit('/').next() == Some(DATA_JSON)
}

/// Whether everything beside `data_json` in its folder is under `files/`,
/// as an export lays it out.
fn is_export_layout(files: &[String], data_json: &str, root: &str) -> bool {
    let prefix = if root.is_empty() {
        String::new()
    } else {
        format!("{root}/")
    };
    let files_prefix = format!("{prefix}files/");
    files
        .iter()
        .filter(|k| k.starts_with(&prefix) && k.as_str() != data_json)
        .all(|k| k.starts_with(&files_prefix))
}

fn join(root: &str, name: &str) -> String {
    if root.is_empty() {
        name.to_string()
    } else {
        format!("{root}/{name}")
    }
}

/// `bsexport://{export}/{kind}/{id}`.
fn ref_href(part: usize, kind: &str, id: i64) -> String {
    format!("{SCHEME}://{part}/{kind}/{id}")
}

/// `[[bsexport:kind:id]]` as `bsexport://{part}/kind/id`. Anything else
/// that starts the same way stays as written.
pub(crate) fn rewrite_refs(text: &str, part: usize) -> String {
    const OPEN: &str = "[[bsexport:";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        let after = &rest[at + OPEN.len()..];
        match read_ref(after) {
            Some((kind, id, len)) => {
                out.push_str(&ref_href(part, kind, id));
                rest = &after[len..];
            }
            None => {
                out.push_str(OPEN);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `kind:id]]` at the start of `s`: the kind, the id and the length read.
fn read_ref(s: &str) -> Option<(&str, i64, usize)> {
    let kind_len = s.bytes().take_while(u8::is_ascii_lowercase).count();
    if kind_len == 0 || kind_len > 20 || s.as_bytes().get(kind_len) != Some(&b':') {
        return None;
    }
    let digits = &s[kind_len + 1..];
    let id_len = digits.bytes().take_while(u8::is_ascii_digit).count();
    if id_len == 0 || id_len > 18 || !digits[id_len..].starts_with("]]") {
        return None;
    }
    let id = digits[..id_len].parse().ok()?;
    Some((&s[..kind_len], id, kind_len + 1 + id_len + 2))
}

/// Whether `text` holds `href` whole: `bsexport://0/attachment/5` isn't in
/// `bsexport://0/attachment/52`.
fn mentions(text: &str, href: &str) -> bool {
    text.match_indices(href).any(|(at, _)| {
        !text[at + href.len()..]
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_digit())
    })
}

/// Text with control characters removed.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// A title as the planner will hold it.
fn title_of(name: Option<&str>) -> String {
    fit_title(&clean(name.unwrap_or(""))).0
}

/// HTML as plain text, one space between blocks.
fn plain_text(html: &str) -> String {
    let document = html::read_document(
        html,
        &[],
        &html::DefaultDialect,
        &DefaultResolver,
        &mut Notes::default(),
    );
    clean(&ir::blocks_text(&document.blocks).join(" "))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Text Markdown reads as written: ASCII punctuation escaped.
fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// What the name Review lists a file attachment under: its name, with the
/// stored file's extension when the name lacks it.
fn attachment_label(name: Option<&str>, file: &str) -> String {
    let name = clean(name.unwrap_or("")).trim().to_string();
    let ext = file
        .rsplit_once('.')
        .map(|(_, e)| e)
        .filter(|e| !e.is_empty() && e.len() <= 10 && e.chars().all(|c| c.is_ascii_alphanumeric()));
    match ext {
        _ if name.is_empty() => file.to_string(),
        Some(ext)
            if !name
                .to_lowercase()
                .ends_with(&format!(".{}", ext.to_lowercase())) =>
        {
            format!("{name}.{ext}")
        }
        _ => name,
    }
}

/// What a page's conversion needs beyond its [`SourcePage`].
#[derive(Debug, Default)]
struct PageExtra {
    /// The export it came from, by position.
    part: usize,
    /// File attachments, by id.
    files: Vec<i64>,
    /// Link attachments: name and address.
    links: Vec<(String, String)>,
}

/// BookStack exports, read through the seam.
pub struct BookstackSource {
    exports: Vec<Export>,
    /// Page titles by id, per export, for includes.
    titles: Vec<HashMap<i64, String>>,
    /// By page key.
    extras: HashMap<String, PageExtra>,
}

impl BookstackSource {
    pub fn new(exports: Exports) -> Self {
        BookstackSource {
            exports: exports.0,
            titles: Vec::new(),
            extras: HashMap::new(),
        }
    }
}

/// The tree as it is read.
#[derive(Default)]
struct Layout {
    pages: Vec<SourcePage>,
    collections: Vec<PlannedCollection>,
    top: Vec<usize>,
    refs: SourceRefs,
    keys: HashSet<String>,
    extras: HashMap<String, PageExtra>,
}

/// One export being read into the layout.
struct Reading<'a> {
    layout: &'a mut Layout,
    part: usize,
    root: String,
    instance: String,
    /// Pages and chapters without an id, numbered as read.
    unnumbered: usize,
    titles: HashMap<i64, String>,
}

/// A book's direct child: a page or a chapter.
enum Child {
    Page(data::Page),
    Chapter(data::Chapter),
}

impl Child {
    fn priority(&self) -> f64 {
        match self {
            Child::Page(p) => p.priority,
            Child::Chapter(c) => c.priority,
        }
        .unwrap_or(0.0)
    }
}

/// Stable, low to high, as BookStack sorts a book's children.
fn by_priority<T>(items: &mut [T], priority: impl Fn(&T) -> f64) {
    items.sort_by(|a, b| {
        priority(a)
            .partial_cmp(&priority(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

impl Reading<'_> {
    /// A key no other page in the import has.
    fn key(&mut self, kind: &str, id: Option<i64>) -> String {
        let base = match id {
            Some(id) => format!("bookstack:{}:{kind}:{id}", self.instance),
            None => {
                self.unnumbered += 1;
                format!(
                    "bookstack:{}:{kind}@{}.{}",
                    self.instance, self.part, self.unnumbered
                )
            }
        };
        let mut key = base.clone();
        let mut n = 1;
        while self.layout.keys.contains(&key) {
            n += 1;
            key = format!("{base}#{n}");
        }
        self.layout.keys.insert(key.clone());
        key
    }

    fn add_ref(&mut self, kind: &str, id: Option<i64>, target: SourceRef) {
        if let Some(id) = id {
            self.layout
                .refs
                .targets
                .insert(ref_href(self.part, kind, id), target);
        }
    }

    /// Every image and attachment in the export, by id: a page may show an
    /// image another page lists.
    fn files(&mut self, pages: &[&data::Page]) {
        for page in pages {
            for image in page.images.iter().flatten() {
                let Some(file) = image.file.as_deref().filter(|f| !f.is_empty()) else {
                    continue;
                };
                let key = join(&self.root, &format!("files/{file}"));
                self.add_ref("image", image.id, SourceRef::Image(key));
            }
            for attachment in page.attachments.iter().flatten() {
                let target = match (&attachment.file, &attachment.link) {
                    (Some(file), _) if !file.is_empty() => {
                        SourceRef::Attachment(attachment_label(attachment.name.as_deref(), file))
                    }
                    (_, Some(link)) if !link.trim().is_empty() => {
                        SourceRef::Web(link.trim().to_string())
                    }
                    _ => continue,
                };
                self.add_ref("attachment", attachment.id, target);
            }
        }
    }

    fn page(&mut self, page: data::Page) -> usize {
        let key = self.key("page", page.id);
        let title = title_of(page.name.as_deref());
        if let Some(id) = page.id {
            self.titles.insert(id, title.clone());
        }
        let markdown = page.markdown.filter(|m| !m.trim().is_empty());
        let html = page.html.filter(|h| !h.trim().is_empty());
        let size = markdown.as_ref().or(html.as_ref()).map_or(0, String::len) as u64;
        let (body, error) = if size > MAX_MARKDOWN_BYTES {
            (
                Body::Empty,
                Some(format!(
                    "This page's content is larger than {} MB.",
                    MAX_MARKDOWN_BYTES / (1024 * 1024)
                )),
            )
        } else {
            match (markdown, html) {
                (Some(markdown), _) => (Body::Markdown(markdown), None),
                (None, Some(html)) => (Body::Html(html), None),
                (None, None) => (Body::Empty, None),
            }
        };
        let mut extra = PageExtra {
            part: self.part,
            ..PageExtra::default()
        };
        for attachment in page.attachments.into_iter().flatten() {
            match (attachment.file, attachment.link) {
                (Some(file), _) if !file.is_empty() => {
                    if let Some(id) = attachment.id {
                        extra.files.push(id);
                    }
                }
                (_, Some(link)) => {
                    let link = link.trim().to_string();
                    let lower = link.to_ascii_lowercase();
                    if lower.starts_with("https://") || lower.starts_with("http://") {
                        let name = clean(attachment.name.as_deref().unwrap_or(""))
                            .trim()
                            .to_string();
                        extra.links.push((name, link));
                    }
                }
                _ => {}
            }
        }
        let tagged = page.tags.is_some_and(|t| !t.is_empty());
        let index = self.layout.pages.len();
        self.add_ref("page", page.id, SourceRef::Page(index));
        self.layout.extras.insert(key.clone(), extra);
        self.layout.pages.push(SourcePage {
            key,
            title,
            kind: PageKind::Page,
            body,
            error,
            metadata_not_imported: if tagged {
                vec!["tags".to_string()]
            } else {
                Vec::new()
            },
            ..SourcePage::default()
        });
        index
    }

    fn chapter(&mut self, chapter: data::Chapter) -> usize {
        let key = self.key("chapter", chapter.id);
        let body = match chapter.description_html {
            Some(html) if !plain_text(&html).is_empty() => {
                if html.len() as u64 > MAX_MARKDOWN_BYTES {
                    Body::Empty
                } else {
                    Body::Html(html)
                }
            }
            _ => Body::Empty,
        };
        let tagged = chapter.tags.is_some_and(|t| !t.is_empty());
        let index = self.layout.pages.len();
        self.add_ref("chapter", chapter.id, SourceRef::Page(index));
        self.layout.extras.insert(
            key.clone(),
            PageExtra {
                part: self.part,
                ..PageExtra::default()
            },
        );
        self.layout.pages.push(SourcePage {
            key,
            title: title_of(chapter.name.as_deref()),
            group: true,
            kind: PageKind::Section,
            body,
            metadata_not_imported: if tagged {
                vec!["tags".to_string()]
            } else {
                Vec::new()
            },
            ..SourcePage::default()
        });
        let mut pages = chapter.pages.unwrap_or_default();
        by_priority(&mut pages, |p| p.priority.unwrap_or(0.0));
        let children: Vec<usize> = pages.into_iter().map(|p| self.page(p)).collect();
        self.layout.pages[index].children = children;
        index
    }

    /// A collection the export's top-level pages go in.
    fn collection(&mut self, name: Option<&str>, description: Option<&str>) -> usize {
        let description = description
            .map(plain_text)
            .filter(|d| !d.is_empty())
            .map(|d| d.chars().take(MAX_DESCRIPTION_CHARS).collect());
        self.layout.collections.push(PlannedCollection {
            name: title_of(name),
            slug: None,
            icon: None,
            color: None,
            description,
            source_uuid: None,
        });
        self.layout.collections.len() - 1
    }

    fn top(&mut self, page: usize, collection: usize) {
        self.layout.pages[page].collection = Some(collection);
        self.layout.top.push(page);
    }

    fn export(&mut self, data: data::DataJson) {
        if let Some(book) = data.book {
            let chapters = book.chapters.unwrap_or_default();
            let pages = book.pages.unwrap_or_default();
            let all: Vec<&data::Page> = pages
                .iter()
                .chain(chapters.iter().flat_map(|c| c.pages.iter().flatten()))
                .collect();
            self.files(&all);
            let collection =
                self.collection(book.name.as_deref(), book.description_html.as_deref());
            self.add_ref("book", book.id, SourceRef::Collection(collection));
            // Pages before chapters, then by priority: BookStack's order,
            // ties included.
            let mut children: Vec<Child> = pages
                .into_iter()
                .map(Child::Page)
                .chain(chapters.into_iter().map(Child::Chapter))
                .collect();
            by_priority(&mut children, Child::priority);
            for child in children {
                let index = match child {
                    Child::Page(page) => self.page(page),
                    Child::Chapter(chapter) => self.chapter(chapter),
                };
                self.top(index, collection);
            }
        } else if let Some(chapter) = data.chapter {
            let all: Vec<&data::Page> = chapter.pages.iter().flatten().collect();
            self.files(&all);
            let collection = self.collection(chapter.name.as_deref(), None);
            let index = self.chapter(chapter);
            self.top(index, collection);
        } else if let Some(page) = data.page {
            self.files(&[&page]);
            let collection = self.collection(page.name.as_deref(), None);
            let index = self.page(page);
            self.top(index, collection);
        }
    }
}

impl DocsSource for BookstackSource {
    fn kind(&self) -> ProfileKind {
        ProfileKind::Bookstack
    }

    fn read(&mut self, input: &mut SourceInput<'_>) -> Result<SourceTree, Refused> {
        let SourceInput::Archive { files, .. } = input;
        let exports = std::mem::take(&mut self.exports);
        let single = exports.len() == 1;
        let root = match exports.as_slice() {
            [only] => only.root.clone(),
            _ => String::new(),
        };
        let mut layout = Layout {
            refs: SourceRefs {
                schemes: vec![SCHEME],
                ..SourceRefs::default()
            },
            ..Layout::default()
        };
        for (part, export) in exports.into_iter().enumerate() {
            let instance: String = export
                .data
                .instance
                .as_ref()
                .and_then(|i| i.id.as_deref())
                .unwrap_or("")
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .take(MAX_INSTANCE_CHARS)
                .collect();
            let mut reading = Reading {
                layout: &mut layout,
                part,
                root: export.root,
                instance: if instance.is_empty() {
                    "-".to_string()
                } else {
                    instance
                },
                unnumbered: 0,
                titles: HashMap::new(),
            };
            reading.export(export.data);
            let titles = std::mem::take(&mut reading.titles);
            self.titles.push(titles);
            if layout.pages.len() > MAX_PAGES {
                return Err(super::tree::refused_too_many(layout.pages.len()));
            }
        }
        self.extras = std::mem::take(&mut layout.extras);
        let site_name = single
            .then(|| layout.collections.first().map(|c| c.name.clone()))
            .flatten();
        Ok(SourceTree {
            root,
            site_name,
            default_collections: CollectionMode::AsSource,
            collections: Some(layout.collections),
            pages: layout.pages,
            top: layout.top,
            files: files.to_vec(),
            left_out: Vec::new(),
            workspace_uuid: None,
            refs: layout.refs,
        })
    }

    fn convert(&self, page: &SourcePage, resolver: &dyn Resolver, notes: &mut Notes) -> Document {
        let Some(extra) = self.extras.get(&page.key) else {
            return Document::default();
        };
        let part = extra.part;
        let (mut document, raw) = match &page.body {
            Body::Markdown(text) => {
                let mut text = rewrite_refs(text, part);
                text = self.includes(&text, part, resolver, notes);
                // BookStack reads no front matter: a page opening with a
                // rule keeps it.
                if text.starts_with("---") || text.starts_with("+++") {
                    text.insert(0, '\n');
                }
                let parsed = parse::parse_with_dialect(
                    &text,
                    ParseOptions::default(),
                    resolver,
                    &BookstackDialect,
                );
                notes.merge(&parsed.notes);
                (parsed.document, text)
            }
            Body::Html(source) => {
                let source = rewrite_refs(source, part);
                let document =
                    html::read_document(&source, &[], &BookstackDialect, resolver, notes);
                (document, source)
            }
            Body::Csv(_) | Body::Empty => (Document::default(), String::new()),
        };

        // File attachments the body doesn't link are listed all the same.
        for id in &extra.files {
            let href = ref_href(part, "attachment", *id);
            if !mentions(&raw, &href) {
                // Asking registers it in the page's report as left out.
                let _ = resolver.link(&href);
                notes.add(NoteCode::AttachmentsLeftOut);
            }
        }
        if !extra.links.is_empty() {
            document.blocks.push(Block::Paragraph(vec![Inline::marked(
                ATTACHMENTS_HEADING,
                Marks::strong(),
            )]));
            let items = extra
                .links
                .iter()
                .map(|(name, url)| {
                    let text = if name.is_empty() {
                        url.clone()
                    } else {
                        name.clone()
                    };
                    ListItem {
                        blocks: vec![Block::Paragraph(vec![Inline::marked(
                            text,
                            Marks {
                                link: Some(Link {
                                    href: url.clone(),
                                    title: None,
                                }),
                                ..Marks::default()
                            },
                        )])],
                    }
                })
                .collect();
            document.blocks.push(Block::BulletList(items));
        }
        document
    }
}

impl BookstackSource {
    /// `{{@12}}` and `{{@12#bkmrk-x}}` in Markdown as a link "Included from
    /// {title}" when page 12 is in the import; otherwise the text stays.
    fn includes(
        &self,
        text: &str,
        part: usize,
        resolver: &dyn Resolver,
        notes: &mut Notes,
    ) -> String {
        const OPEN: &str = "{{@";
        let Some(titles) = self.titles.get(part) else {
            return text.to_string();
        };
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find(OPEN) {
            out.push_str(&rest[..at]);
            let after = &rest[at + OPEN.len()..];
            let tag = after
                .find("}}")
                .filter(|end| *end <= 200)
                .map(|end| &after[..end])
                .filter(|tag| !tag.contains('\n'));
            let id = tag.and_then(|tag| {
                let tag = tag.strip_prefix(char::is_whitespace).unwrap_or(tag);
                let digits = tag.bytes().take_while(u8::is_ascii_digit).count();
                tag[..digits].parse::<i64>().ok()
            });
            let linked = id.and_then(|id| {
                let title = titles.get(&id)?;
                let href = ref_href(part, "page", id);
                resolver.link(&href)?;
                Some((title, href))
            });
            match (tag, linked) {
                (Some(tag), Some((title, href))) => {
                    notes.add(NoteCode::IncludesAsLinks);
                    let _ = write!(out, "[Included from {}]({href})", escape_markdown(title));
                    rest = &after[tag.len() + 2..];
                }
                _ => {
                    out.push_str(OPEN);
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }
}
