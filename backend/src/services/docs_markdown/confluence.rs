//! Confluence's HTML space export as a documentation source.
//!
//! What Confluence Data Center 10.2 writes (Space tools > Content tools >
//! Export > HTML, Normal or Custom) is a zip holding one folder per space,
//! named by its key:
//!
//! - `index.html`, the space details: `<title>KEY (Name)</title>`, a table
//!   of key, name and description, and an "Available Pages" list of the
//!   page tree in the space's own order, nested `<ul>`s with every child in
//!   a `<ul>` of its own. The homepage carries the `home_page_16.png` icon.
//! - A file per page beside it: `Title-Words_123.html`, or `123.html` when
//!   the title has characters a file name can't take. The content sits in
//!   `#main-content`; breadcrumbs, the title, "Created by", the Attachments
//!   and Comments sections and the footer are page chrome around it.
//! - `attachments/{pageId}/{attachmentId}.{ext}`, Confluence's icons under
//!   `images/icons/` and a stylesheet.
//!
//! An XML space backup (`entities.xml`) and a folder of pages without its
//! `index.html` are refused with how to export again.
//!
//! The tree follows the index, the homepage first with its children beside
//! it, then the other top-level pages; a page the index doesn't list goes
//! under its nearest breadcrumb in the import. Each space is a collection.
//! Page bodies go through [`ConfluenceDialect`], and links in Confluence's
//! own URL forms (`viewpage.action?pageId=`, `/display/KEY/Title`,
//! `/spaces/KEY/pages/ID`, `/download/attachments/...`) are pointed at the
//! archive's files before the planner resolves them, so they reach the
//! imported page or attachment as a relative link does.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::html::{self, parse_page, Dialect, ElementView, Rewrite, Selector};
use super::ir::{Block, Document, Inline, ListItem};
use super::links::{join_normalised, parent, percent_decode};
use super::profiles::ProfileKind;
use super::source::{Body, DocsSource, SourceInput, SourcePage, SourceRef, SourceRefs, SourceTree};
use super::tree::{natural_cmp, CollectionMode, PlannedCollection};
use super::{NoteCode, Notes, Resolver};
use crate::services::imports::refusal::Refused;
use crate::services::uploaded_archive::Text;

/// Largest `index.html` read.
const MAX_INDEX_BYTES: u64 = 10 * 1024 * 1024;
/// Largest page read: a page's HTML carries its chrome too.
const MAX_PAGE_BYTES: u64 = 5 * 1024 * 1024;
/// `index.html` files read to find spaces.
const MAX_INDEX_CANDIDATES: usize = 50;
/// Loose HTML files read to recognise pages without their index.
const MAX_PAGE_SAMPLES: usize = 3;

/// Where a page's content sits, in order of preference.
const CONTENT: [Selector; 2] = [Selector::Id("main-content"), Selector::Id("content")];

/// The refusal for an XML space backup.
pub fn refused_xml() -> Refused {
    Refused::new(
        "docs_import_confluence_xml",
        "This is a Confluence XML export. Export the space as HTML instead.",
    )
}

/// The refusal for Confluence pages without the export's `index.html`.
pub fn refused_no_index() -> Refused {
    Refused::new(
        "docs_import_confluence_no_index",
        "This looks like a Confluence export, but its index.html is missing. Export the space again with the HTML option.",
    )
}

// Detection.

/// Whether `files` hold a Confluence export: `Some(Ok(base))` with the
/// folder its spaces sit under, `Some(Err)` for an export to refuse (an XML
/// backup, pages without their index), `None` for anything else.
pub fn detect(
    files: &[String],
    read: &mut dyn FnMut(&str) -> Option<String>,
) -> Option<Result<String, Refused>> {
    if is_xml_backup(files) {
        return Some(Err(refused_xml()));
    }
    let roots = space_roots(files, "", read);
    if !roots.is_empty() {
        return Some(Ok(common_folder(&roots)));
    }
    // Pages, not an index: an exported space with no pages is no space,
    // not one whose index is missing.
    let samples: Vec<&String> = files
        .iter()
        .filter(|k| is_html(k) && !is_index(k))
        .take(MAX_PAGE_SAMPLES)
        .collect();
    if samples
        .into_iter()
        .any(|k| read(k).is_some_and(|t| is_confluence_page(&t)))
    {
        return Some(Err(refused_no_index()));
    }
    None
}

/// `entities.xml` at the root or beside `exportDescriptor.properties`.
fn is_xml_backup(files: &[String]) -> bool {
    files.iter().any(|k| {
        let (dir, name) = k.rsplit_once('/').unwrap_or(("", k));
        name == "entities.xml"
            && (dir.is_empty()
                || files
                    .iter()
                    .any(|o| *o == format!("{dir}/exportDescriptor.properties")))
    })
}

fn is_html(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.ends_with(".html") || lower.ends_with(".htm")
}

fn is_index(key: &str) -> bool {
    key.rsplit('/').next() == Some("index.html")
}

/// The text of a page Confluence exported: its content root and its
/// generator's marks.
fn is_confluence_page(text: &str) -> bool {
    text.contains("id=\"main-content\"")
        && (text.contains("Document generated by Confluence")
            || (text.contains("id=\"breadcrumbs\"") && text.contains("atlassian")))
}

/// An `index.html` Confluence wrote: the space details and its page list.
fn is_space_index(text: &str) -> bool {
    text.contains("pageSectionTitle") && (text.contains("Confluence") || text.contains("atlassian"))
}

/// The folders under `base` holding a space: an `index.html` Confluence
/// wrote, with pages beside it. In name order.
fn space_roots(
    files: &[String],
    base: &str,
    read: &mut dyn FnMut(&str) -> Option<String>,
) -> Vec<String> {
    let html_dirs: HashSet<&str> = files
        .iter()
        .filter(|k| is_html(k) && !is_index(k))
        .map(|k| parent(k))
        .collect();
    let mut roots: Vec<String> = files
        .iter()
        .filter(|k| is_index(k) && under(base, k) && html_dirs.contains(parent(k)))
        .take(MAX_INDEX_CANDIDATES)
        .filter(|k| read(k).is_some_and(|t| is_space_index(&t)))
        .map(|k| parent(k).to_string())
        .collect();
    roots.sort_by(|a, b| natural_cmp(a, b));
    roots.dedup();
    roots
}

fn under(base: &str, key: &str) -> bool {
    base.is_empty() || key.strip_prefix(base).is_some_and(|r| r.starts_with('/'))
}

/// The deepest folder every root sits in (a root itself when there is one).
fn common_folder(roots: &[String]) -> String {
    let Some(first) = roots.first() else {
        return String::new();
    };
    if roots.len() == 1 {
        return first.clone();
    }
    let mut common: Vec<&str> = parent(first).split('/').filter(|s| !s.is_empty()).collect();
    for root in &roots[1..] {
        let segments: Vec<&str> = parent(root).split('/').filter(|s| !s.is_empty()).collect();
        let same = common
            .iter()
            .zip(&segments)
            .take_while(|(a, b)| a == b)
            .count();
        common.truncate(same);
    }
    common.join("/")
}

// The space's index.

/// A space as its `index.html` describes it.
#[derive(Debug, Default)]
struct SpaceIndex {
    key: String,
    name: String,
    description: Option<String>,
    /// The page tree, top level first.
    pages: Vec<IndexNode>,
}

#[derive(Debug, Default)]
struct IndexNode {
    href: String,
    title: String,
    /// Carries the homepage icon.
    home: bool,
    children: Vec<IndexNode>,
}

/// Read a space's `index.html`; `folder` is its folder's name, the space
/// key when the index doesn't say.
fn read_index(text: &str, folder: &str) -> SpaceIndex {
    let mut index = SpaceIndex {
        key: folder.to_string(),
        name: folder.to_string(),
        ..SpaceIndex::default()
    };
    let Some(page) = parse_page(text) else {
        return index;
    };
    // `<title>KEY (Name)</title>`, in every language.
    if let Some(title) = TITLE.captures(text).map(|c| decode_entities(c[1].trim())) {
        if let Some((key, name)) = title.split_once(" (") {
            if let Some(name) = name.strip_suffix(')') {
                if !key.trim().is_empty() {
                    index.key = key.trim().to_string();
                }
                if !name.trim().is_empty() {
                    index.name = name.trim().to_string();
                }
            }
        }
    }
    // The details table: key, name, description, by position (the labels
    // are in the exporter's language).
    if let Some(details) = page.find(&Selector::Id("main-content")) {
        let cells: Vec<String> = descendants(&details)
            .into_iter()
            .filter(|e| e.tag() == "td")
            .map(|e| e.text())
            .collect();
        if let Some(description) = cells.get(2).filter(|d| !d.is_empty()) {
            index.description = Some(description.clone());
        }
    }
    // The page list: the `<ul>`s of the section that holds one.
    for section in descendants(&page)
        .into_iter()
        .filter(|e| e.has_class("pageSection"))
    {
        let lists: Vec<ElementView> = section
            .children()
            .into_iter()
            .filter(|e| e.tag() == "ul")
            .collect();
        if lists.is_empty() {
            continue;
        }
        for list in lists {
            index.pages.extend(list_nodes(&list));
        }
        break;
    }
    index
}

/// The pages of one `<ul>`, each with its children.
fn list_nodes(list: &ElementView) -> Vec<IndexNode> {
    list.children()
        .into_iter()
        .filter(|e| e.tag() == "li")
        .filter_map(|li| {
            let children = li.children();
            let link = children.iter().find(|e| e.tag() == "a")?;
            let home = children.iter().any(|e| {
                e.tag() == "img" && e.attr("src").is_some_and(|s| s.contains("home_page"))
            });
            let nested = children
                .iter()
                .filter(|e| e.tag() == "ul")
                .flat_map(list_nodes)
                .collect();
            Some(IndexNode {
                href: link.attr("href").unwrap_or_default(),
                title: link.text(),
                home,
                children: nested,
            })
        })
        .collect()
}

/// Every element under `root`, in document order. Iterative.
fn descendants(root: &ElementView) -> Vec<ElementView> {
    let mut out = Vec::new();
    let mut stack: Vec<ElementView> = root.children().into_iter().rev().collect();
    while let Some(element) = stack.pop() {
        stack.extend(element.children().into_iter().rev());
        out.push(element);
    }
    out
}

/// What a page's own HTML says about where it sits.
#[derive(Debug, Default)]
struct PageChrome {
    /// The title without the "Space : " prefix.
    title: Option<String>,
    /// Breadcrumb links, from the top, without the space's index.
    crumbs: Vec<String>,
}

fn read_chrome(text: &str, space_name: &str) -> PageChrome {
    let Some(page) = parse_page(text) else {
        return PageChrome::default();
    };
    let title = page
        .find(&Selector::Id("title-text"))
        .map(|t| t.text())
        .or_else(|| TITLE.captures(text).map(|c| decode_entities(c[1].trim())))
        .map(|t| {
            t.strip_prefix(&format!("{space_name} : "))
                .map(str::to_string)
                .unwrap_or(t)
        })
        .filter(|t| !t.trim().is_empty());
    let crumbs = page
        .find(&Selector::Id("breadcrumbs"))
        .map(|b| {
            descendants(&b)
                .into_iter()
                .filter(|e| e.tag() == "a")
                .filter_map(|a| a.attr("href"))
                .filter(|h| h != "index.html")
                .collect()
        })
        .unwrap_or_default();
    PageChrome { title, crumbs }
}

static PAGE_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|_)(\d+)\.html?$").expect("page id pattern"));
static OWN_ATTACHMENTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"id="attachments"[\s\S]*?href="attachments/(\d+)/"#).expect("attachments pattern")
});
/// A link into the attachments folder, with the name Confluence gives the
/// file: as `data-linked-resource-default-alias`, or as the text of a link
/// in the page's Attachments section.
static ATTACHMENT_ALIAS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?:href|src)="attachments/(\d+)/([^"/]+)"[^>]*?data-linked-resource-default-alias="([^"]+)""#,
    )
        .expect("alias pattern")
});
static ATTACHMENT_LISTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<a href="attachments/(\d+)/([^"/]+)">([^<]+)</a>"#).expect("listed pattern")
});
/// A page's `<title>`, which the element reader leaves out as chrome.
static TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<title>\s*([^<]*?)\s*</title>").expect("title pattern"));
static BASE_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"data-base-url="([^"]+)""#).expect("base url pattern"));

/// The page id in a file name (`Title_123.html`, `123.html`), else in the
/// page's own Attachments section.
fn page_id(key: &str, text: &str) -> Option<String> {
    let name = key.rsplit('/').next().unwrap_or(key);
    PAGE_ID
        .captures(name)
        .or_else(|| OWN_ATTACHMENTS.captures(text))
        .map(|c| c[1].to_string())
}

/// `&amp;` and the other entities Confluence writes in attributes and
/// link text.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A file name as a title: `Install-Guide_123` reads "Install Guide".
fn title_from_file(key: &str) -> String {
    let stem = super::links::file_stem(key);
    let stem = match stem.rsplit_once('_') {
        Some((name, id)) if !name.is_empty() && id.chars().all(|c| c.is_ascii_digit()) => name,
        _ => stem,
    };
    stem.replace('-', " ").trim().to_string()
}

// The source.

/// A Confluence HTML export: one or more spaces.
pub struct Confluence {
    /// The folder detection found the spaces under.
    base: String,
    /// Page files by Confluence page id.
    by_id: HashMap<String, String>,
    /// Page files by space key and title, both lowercased.
    by_title: HashMap<(String, String), String>,
    /// Attachment files by page id and the file's name.
    attachments: HashMap<(String, String), String>,
    /// The hosts the export names as its own (`data-base-url`), lowercased.
    hosts: HashSet<String>,
    /// Pages with an id, by id, to their index in the tree.
    pages_by_id: HashMap<String, usize>,
    /// Page ids by the page's file.
    ids_by_key: HashMap<String, String>,
    /// Attachment files, by key, to their page id and attachment id.
    attachment_ids: HashMap<String, (String, String)>,
    /// The name Confluence gives each attachment file, by key.
    attachment_names: HashMap<String, String>,
}

/// The scheme of the destinations this source writes for the planner:
/// `confluence://page/{id}`, `confluence://file/{page}/{attachment}` and
/// `confluence://image/{page}/{attachment}`.
const SCHEME: &str = "confluence";

impl Confluence {
    pub fn new(base: String) -> Self {
        Confluence {
            base,
            by_id: HashMap::new(),
            by_title: HashMap::new(),
            attachments: HashMap::new(),
            hosts: HashSet::new(),
            pages_by_id: HashMap::new(),
            ids_by_key: HashMap::new(),
            attachment_ids: HashMap::new(),
            attachment_names: HashMap::new(),
        }
    }

    /// Every page and attachment as a destination the planner resolves.
    fn refs(&self) -> SourceRefs {
        let mut targets = HashMap::new();
        for (id, page) in &self.pages_by_id {
            targets.insert(format!("{SCHEME}://page/{id}"), SourceRef::Page(*page));
        }
        for (key, (page, attachment)) in &self.attachment_ids {
            let name = self
                .attachment_names
                .get(key)
                .cloned()
                .unwrap_or_else(|| key.rsplit('/').next().unwrap_or(key).to_string());
            targets.insert(
                format!("{SCHEME}://file/{page}/{attachment}"),
                SourceRef::Attachment(name),
            );
            targets.insert(
                format!("{SCHEME}://image/{page}/{attachment}"),
                SourceRef::Image(key.clone()),
            );
        }
        SourceRefs {
            schemes: vec![SCHEME],
            targets,
        }
    }
}

/// One space being read into the tree.
struct SpaceRead<'a> {
    root: &'a str,
    space: &'a SpaceIndex,
    collection: usize,
    /// The space's page files by lowercased key: the key, and its page
    /// once read.
    files: HashMap<String, (String, Option<usize>)>,
    /// Attachment files by the folder they sit in.
    attachments: HashMap<String, Vec<String>>,
}

impl SpaceRead<'_> {
    fn page_of(&self, key: &str) -> Option<usize> {
        self.files
            .get(&key.to_lowercase())
            .and_then(|(_, page)| *page)
    }
}

impl DocsSource for Confluence {
    fn kind(&self) -> ProfileKind {
        ProfileKind::Confluence
    }

    fn read(&mut self, input: &mut SourceInput<'_>) -> Result<SourceTree, Refused> {
        let SourceInput::Archive { archive, files } = input;
        let mut read_text =
            |key: &str, max: u64| archive.read_text(key, max).map_err(|e| e.to_string());
        let roots = space_roots(files, &self.base, &mut |k| {
            read_text(k, MAX_INDEX_BYTES).ok().map(|t| t.text)
        });
        if roots.is_empty() {
            return Err(refused_no_index());
        }

        let mut pages: Vec<SourcePage> = Vec::new();
        let mut top: Vec<usize> = Vec::new();
        let mut collections = Vec::new();
        let mut names = Vec::new();
        for root in &roots {
            let index_key = join(root, "index.html");
            let index_text = read_text(&index_key, MAX_INDEX_BYTES)
                .map(|t| t.text)
                .unwrap_or_default();
            let folder = root.rsplit('/').next().unwrap_or(root);
            let space = read_index(&index_text, folder);
            collections.push(PlannedCollection {
                name: space.name.clone(),
                slug: None,
                icon: None,
                color: None,
                description: space.description.clone(),
                source_uuid: None,
            });
            names.push(space.name.clone());
            let mut reading = SpaceRead {
                root,
                space: &space,
                collection: collections.len() - 1,
                files: files
                    .iter()
                    .filter(|k| parent(k) == root.as_str() && is_html(k) && !is_index(k))
                    .map(|k| (k.to_lowercase(), (k.clone(), None)))
                    .collect(),
                attachments: attachment_folders(files, root),
            };
            for (id, keys) in &reading.attachments {
                for key in keys {
                    let attachment = super::links::file_stem(key).to_string();
                    self.attachment_ids
                        .insert(key.clone(), (id.clone(), attachment));
                }
            }
            let space_top = self.read_space(&mut reading, &mut read_text, &mut pages);
            top.extend(space_top);
        }

        Ok(SourceTree {
            root: if roots.len() == 1 {
                roots[0].clone()
            } else {
                self.base.clone()
            },
            site_name: (names.len() == 1).then(|| names[0].clone()),
            default_collections: CollectionMode::AsSource,
            collections: Some(collections),
            pages,
            top,
            files: files.to_vec(),
            left_out: Vec::new(),
            workspace_uuid: None,
            refs: self.refs(),
        })
    }

    fn convert(&self, page: &SourcePage, resolver: &dyn Resolver, notes: &mut Notes) -> Document {
        let Body::Html(raw) = &page.body else {
            return Document::default();
        };
        let mut document = html::read_document(raw, &CONTENT, &ConfluenceDialect, resolver, notes);
        let dir = page.file.as_deref().map(parent).unwrap_or("");
        self.point_at_archive(&mut document, dir);
        document
    }
}

impl Confluence {
    /// Read one space's pages into `pages`; returns its top-level pages in
    /// order.
    fn read_space(
        &mut self,
        reading: &mut SpaceRead<'_>,
        read_text: &mut ReadText<'_>,
        pages: &mut Vec<SourcePage>,
    ) -> Vec<usize> {
        let space = reading.space;
        // The index's tree, the homepage first with its children beside it.
        let mut top = Vec::new();
        let mut roots: Vec<&IndexNode> = space.pages.iter().collect();
        if let Some(home) = roots.iter().position(|n| n.home) {
            let home = roots.remove(home);
            top.extend(self.index_page(reading, home, read_text, pages, false));
            for child in &home.children {
                top.extend(self.index_page(reading, child, read_text, pages, true));
            }
        }
        for node in roots {
            top.extend(self.index_page(reading, node, read_text, pages, true));
        }

        // Pages the index doesn't list, under their nearest breadcrumb.
        let mut loose: Vec<String> = reading
            .files
            .values()
            .filter(|(_, page)| page.is_none())
            .map(|(key, _)| key.clone())
            .collect();
        loose.sort_by(|a, b| natural_cmp(a, b));
        let mut waiting: Vec<(usize, Vec<String>)> = Vec::new();
        for key in loose {
            let page = self.new_page(reading, &key, None, read_text, pages);
            let crumbs: Vec<String> = match &pages[page].body {
                Body::Html(text) => read_chrome(text, &space.name)
                    .crumbs
                    .iter()
                    .filter_map(|href| self.page_file(reading, href))
                    .filter(|k| !k.eq_ignore_ascii_case(&key))
                    .collect(),
                _ => Vec::new(),
            };
            waiting.push((page, crumbs));
        }
        // A breadcrumb counts once its page is placed, so no cycle forms.
        let mut placed: HashSet<usize> = (0..pages.len())
            .filter(|p| !waiting.iter().any(|(w, _)| w == p))
            .collect();
        loop {
            let mut progress = false;
            waiting.retain(|(page, crumbs)| {
                let in_import: Vec<usize> =
                    crumbs.iter().filter_map(|k| reading.page_of(k)).collect();
                let above = in_import.iter().rev().find(|p| placed.contains(p)).copied();
                if above.is_none() && !in_import.is_empty() {
                    return true;
                }
                match above {
                    Some(p) => pages[p].children.push(*page),
                    // Nothing above it in the import: the top level.
                    None => top.push(*page),
                }
                placed.insert(*page);
                progress = true;
                false
            });
            if !progress {
                break;
            }
        }
        // What's left waits only on pages waiting on it.
        top.extend(waiting.into_iter().map(|(page, _)| page));

        for page in &top {
            pages[*page].collection = Some(reading.collection);
        }
        top
    }

    /// The page an index entry names, with its children unless the caller
    /// places them; `None` when it names no page in the space's folder, or
    /// one already read.
    fn index_page(
        &mut self,
        reading: &mut SpaceRead<'_>,
        node: &IndexNode,
        read_text: &mut ReadText<'_>,
        pages: &mut Vec<SourcePage>,
        with_children: bool,
    ) -> Option<usize> {
        let key = self.page_file(reading, &node.href)?;
        if reading.page_of(&key).is_some() {
            return None;
        }
        let title = Some(node.title.clone()).filter(|t| !t.trim().is_empty());
        let page = self.new_page(reading, &key, title, read_text, pages);
        if with_children {
            for child in &node.children {
                if let Some(c) = self.index_page(reading, child, read_text, pages, true) {
                    pages[page].children.push(c);
                }
            }
        }
        Some(page)
    }

    /// A page's archive key from a link in the space's folder, when it is
    /// one of the space's pages.
    fn page_file(&self, reading: &SpaceRead<'_>, href: &str) -> Option<String> {
        let path = href.split(['#', '?']).next().unwrap_or("");
        if path.is_empty() || path.contains("://") || path.starts_with('/') {
            return None;
        }
        let key = join_normalised(reading.root, &percent_decode(path))?;
        reading
            .files
            .get(&key.to_lowercase())
            .map(|(key, _)| key.clone())
    }

    /// Read the page at `key` and add it to `pages`.
    fn new_page(
        &mut self,
        reading: &mut SpaceRead<'_>,
        key: &str,
        title: Option<String>,
        read_text: &mut ReadText<'_>,
        pages: &mut Vec<SourcePage>,
    ) -> usize {
        let space = reading.space;
        let mut notes = Notes::default();
        let (body, error) = match read_text(key, MAX_PAGE_BYTES) {
            Ok(text) => {
                if text.lossy {
                    notes.add(NoteCode::NotUtf8);
                }
                (Body::Html(text.text), None)
            }
            Err(e) => (Body::Empty, Some(e)),
        };
        let text = match &body {
            Body::Html(text) => text.as_str(),
            _ => "",
        };
        let title = title
            .or_else(|| read_chrome(text, &space.name).title)
            .unwrap_or_else(|| title_from_file(key));
        let id = page_id(key, text);
        let relative = key
            .strip_prefix(reading.root)
            .map(|k| k.trim_start_matches('/'))
            .unwrap_or(key);
        let mut entries = vec![key.to_string()];
        if let Some(id) = &id {
            self.by_id.insert(id.clone(), key.to_string());
            // Leaving the page out leaves its attachments out.
            if let Some(files) = reading.attachments.get(id) {
                entries.extend(files.iter().cloned());
            }
        }
        self.by_title.insert(
            (space.key.to_lowercase(), title.to_lowercase()),
            key.to_string(),
        );
        for c in ATTACHMENT_ALIAS
            .captures_iter(text)
            .chain(ATTACHMENT_LISTED.captures_iter(text))
        {
            let file = join(reading.root, &format!("attachments/{}/{}", &c[1], &c[2]));
            let name = decode_entities(c[3].trim());
            self.attachment_names
                .entry(file.clone())
                .or_insert_with(|| name.clone());
            self.attachments
                .entry((c[1].to_string(), name))
                .or_insert(file);
        }
        for c in BASE_URL.captures_iter(text) {
            if let Some(host) = host_of(&decode_entities(&c[1])) {
                self.hosts.insert(host);
            }
        }
        pages.push(SourcePage {
            key: format!(
                "confluence:{}:{}",
                space.key,
                id.clone().unwrap_or_else(|| relative.to_string())
            ),
            file: Some(key.to_string()),
            entries,
            title,
            body,
            error,
            notes,
            ..SourcePage::default()
        });
        let page = pages.len() - 1;
        if let Some(id) = id {
            self.ids_by_key.insert(key.to_string(), id.clone());
            self.pages_by_id.insert(id, page);
        }
        reading
            .files
            .insert(key.to_lowercase(), (key.to_string(), Some(page)));
        page
    }

    /// Point links and images at what they name in the export, in this
    /// source's own scheme: a page by its id, an attachment by its page
    /// and id. The planner resolves those to the page, the image or the
    /// attachment's name. Relative links to pages are left as they are: the
    /// planner finds the file.
    fn point_at_archive(&self, document: &mut Document, dir: &str) {
        for_each_destination(document, &mut |href, image| {
            let key = if is_relative(href) {
                let path = href.split(['#', '?']).next().unwrap_or("");
                join_normalised(dir, &percent_decode(path))
            } else {
                self.archive_target(href)
            };
            let Some(key) = key else {
                return;
            };
            if let Some((page, attachment)) = self.attachment_ids.get(&key) {
                let kind = if image { "image" } else { "file" };
                *href = format!("{SCHEME}://{kind}/{page}/{attachment}");
            } else if is_relative(href) {
                // A page by its file: the planner finds it.
            } else if let Some(id) = self.ids_by_key.get(&key) {
                *href = format!("{SCHEME}://page/{id}");
            } else {
                // A page without an id: by its file, as a relative link is.
                *href = format!("/{key}");
            }
        });
    }

    /// The archive key a Confluence URL names: a page by id or by space and
    /// title, or an attachment by page and name. Relative links are left to
    /// the planner; an absolute one only names this export when its host is
    /// the export's own (any host when the export names none).
    fn archive_target(&self, href: &str) -> Option<String> {
        let href = href.trim();
        let href = href.split('#').next().unwrap_or("");
        let (path, query) = href.split_once('?').unwrap_or((href, ""));
        let path = if let Some((scheme, rest)) = path.split_once("://") {
            if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
                return None;
            }
            let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
            if !self.hosts.is_empty() && !self.hosts.contains(&host.to_ascii_lowercase()) {
                return None;
            }
            path
        } else {
            // Root-relative; `//host` is a web link with no scheme.
            path.strip_prefix('/').filter(|p| !p.starts_with('/'))?
        };
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let param = |name: &str| {
            query
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .find(|(k, _)| *k == name)
                .map(|(_, v)| plus_decode(v))
        };
        let by_title = |key: &str, title: &str| {
            self.by_title
                .get(&(key.to_lowercase(), title.to_lowercase()))
                .cloned()
        };
        // `.../pages/viewpage.action?pageId=` or `?spaceKey=&title=`.
        if segments.last() == Some(&"viewpage.action") {
            if let Some(id) = param("pageId") {
                return self.by_id.get(id.trim()).cloned();
            }
            if let (Some(key), Some(title)) = (param("spaceKey"), param("title")) {
                return by_title(&key, &title);
            }
            return None;
        }
        for (i, segment) in segments.iter().enumerate() {
            match *segment {
                // `.../spaces/KEY/pages/ID[/Title]`, Data Center 10 and Cloud.
                "spaces" if segments.get(i + 2) == Some(&"pages") => {
                    let id = segments.get(i + 3)?;
                    return self.by_id.get(*id).cloned();
                }
                // `.../display/KEY/Title+Words`.
                "display" => {
                    let key = segments.get(i + 1)?;
                    let title = segments.get(i + 2..).filter(|t| !t.is_empty())?;
                    return by_title(key, &plus_decode(&title.join("/")));
                }
                // `.../download/attachments/PAGE/NAME` and its thumbnail.
                "download"
                    if matches!(segments.get(i + 1), Some(&"attachments" | &"thumbnails")) =>
                {
                    let page = segments.get(i + 2)?;
                    let name = segments.get(i + 3)?;
                    return self
                        .attachments
                        .get(&(page.to_string(), percent_decode(name)))
                        .cloned();
                }
                _ => {}
            }
        }
        None
    }
}

/// A destination with no scheme and no leading `/`.
fn is_relative(href: &str) -> bool {
    let href = href.trim();
    !href.starts_with('/') && !href.contains("://") && !href.contains(':')
}

/// Reads one archive entry as text, at most `max` bytes.
type ReadText<'a> = dyn FnMut(&str, u64) -> Result<Text, String> + 'a;

/// The files of each page's attachments folder, by page id.
fn attachment_folders(files: &[String], root: &str) -> HashMap<String, Vec<String>> {
    let prefix = join(root, "attachments/");
    let mut folders: HashMap<String, Vec<String>> = HashMap::new();
    for key in files {
        if let Some((id, _)) = key.strip_prefix(&prefix).and_then(|r| r.split_once('/')) {
            folders.entry(id.to_string()).or_default().push(key.clone());
        }
    }
    folders
}

/// `+` as a space, then `%XX` decoded: a query value or a `/display/`
/// title.
fn plus_decode(s: &str) -> String {
    percent_decode(&s.replace('+', " "))
}

/// The host (and port) of an absolute http(s) URL, lowercased.
fn host_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next()?.to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

/// Call `f` with every link destination and image source in the
/// document, to change it; `true` for an image's.
fn for_each_destination(document: &mut Document, f: &mut dyn FnMut(&mut String, bool)) {
    fn inlines(list: &mut [Inline], f: &mut dyn FnMut(&mut String, bool)) {
        for inline in list {
            match inline {
                Inline::Text(text) => {
                    if let Some(link) = &mut text.marks.link {
                        f(&mut link.href, false);
                    }
                }
                Inline::Image(image) => f(&mut image.src, true),
                Inline::HardBreak | Inline::TicketLink { .. } | Inline::Mention { .. } => {}
            }
        }
    }
    fn blocks(list: &mut [Block], f: &mut dyn FnMut(&mut String, bool)) {
        for block in list {
            match block {
                Block::Paragraph(content) | Block::Heading { content, .. } => inlines(content, f),
                Block::Blockquote(inner) => blocks(inner, f),
                Block::BulletList(items) | Block::OrderedList { items, .. } => items_of(items, f),
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
    fn items_of(items: &mut [ListItem], f: &mut dyn FnMut(&mut String, bool)) {
        for item in items {
            blocks(&mut item.blocks, f);
        }
    }
    blocks(&mut document.blocks, f);
}

// The dialect.

/// Confluence's page markup, as Data Center 10.2 exports it.
pub struct ConfluenceDialect;

impl Dialect for ConfluenceDialect {
    fn classify(&self, el: &ElementView) -> Option<Rewrite> {
        let class = |c: &str| el.has_class(c);
        let inside = |c: &'static str| el.closest(&Selector::Class(c)).is_some();
        match el.tag() {
            // The style block a table of contents brings, and other
            // generated styles: never content.
            "style" => return Some(Rewrite::Remove(None)),
            "img" => return image(el),
            "li" if el.parent().is_some_and(|p| p.has_class("inline-task-list")) => {
                return Some(Rewrite::Task {
                    checked: class("checked"),
                });
            }
            "a" if class("confluence-userlink") || class("user-mention") => {
                return Some(Rewrite::Text {
                    text: format!("@{}", el.text()),
                    note: Some(NoteCode::MentionsAsText),
                });
            }
            // A link to a page that doesn't exist.
            "a" if class("createlink") => {
                return Some(Rewrite::Text {
                    text: el.text(),
                    note: Some(NoteCode::LinksUnresolved),
                });
            }
            _ => {}
        }

        // Callouts.
        if class("confluence-information-macro") {
            let label = el
                .find(&Selector::Class("title"))
                .map(|t| t.text())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| callout_label(el).to_string());
            return Some(Rewrite::Callout { label });
        }
        if (class("title") && inside("confluence-information-macro"))
            || class("confluence-information-macro-icon")
        {
            return Some(Rewrite::Remove(None));
        }
        if class("confluence-information-macro-body") {
            return Some(Rewrite::Unwrap(None));
        }

        // Code and preformatted text, before panels: both are panels too.
        if class("code") && class("panel") {
            let pre = el.find(&Selector::Tag("pre"));
            return Some(Rewrite::Code {
                language: pre
                    .and_then(|p| p.attr("data-syntaxhighlighter-params"))
                    .and_then(|p| brush(&p))
                    .and_then(|b| language(&b)),
                title: el
                    .find(&Selector::Class("codeHeader"))
                    .map(|h| h.text())
                    .filter(|t| !t.is_empty()),
            });
        }
        if class("preformatted") {
            return Some(Rewrite::Code {
                language: None,
                title: None,
            });
        }
        if class("panel") {
            let label = el
                .children()
                .into_iter()
                .find(|c| c.has_class("panelHeader"))
                .map(|h| h.text())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "Note".to_string());
            return Some(Rewrite::Callout { label });
        }
        if class("panelHeader") && inside("panel") {
            return Some(Rewrite::Remove(None));
        }
        if class("panelContent") {
            return Some(Rewrite::Unwrap(None));
        }

        // Expand.
        if class("expand-container") {
            return Some(Rewrite::Expand {
                title: el
                    .find(&Selector::Class("expand-control-text"))
                    .map(|t| t.text())
                    .filter(|t| !t.is_empty()),
            });
        }
        if class("expand-control") {
            return Some(Rewrite::Remove(None));
        }
        if class("expand-content") {
            return Some(Rewrite::Unwrap(None));
        }

        if class("status-macro") {
            return Some(Rewrite::Status);
        }

        // Jira. With no Jira link the export holds only an error, no key.
        if class("jim-error-message") {
            return Some(Rewrite::Remove(Some(NoteCode::MacrosRemoved)));
        }
        if class("jira-issue") {
            let key_link = el.find(&Selector::Class("jira-issue-key"));
            let key = el
                .attr("data-jira-key")
                .filter(|k| !k.trim().is_empty())
                .or_else(|| key_link.as_ref().map(|k| k.text()))
                .unwrap_or_default();
            let url = key_link
                .or_else(|| el.find(&Selector::Tag("a")))
                .and_then(|a| a.attr("href"))
                .unwrap_or_default();
            return Some(Rewrite::Jira {
                key,
                summary: el.find(&Selector::Class("summary")).map(|s| s.text()),
                url,
            });
        }

        // Macros with nothing to keep.
        if class("toc-macro")
            || class("client-side-toc-macro")
            || class("plugin_pagetree")
            || class("recently-updated")
            || class("plugin-contributors")
            || class("plugin_attachments_container")
            || (el.tag() == "div"
                && class("error")
                && el.children().iter().any(|c| c.has_class("error")))
        {
            return Some(Rewrite::Remove(Some(NoteCode::MacrosRemoved)));
        }
        if class("confluence-anchor-link") || class("companion-edit-button-placeholder") {
            return Some(Rewrite::Remove(None));
        }

        // Layouts and wrappers.
        if class("contentLayout2") {
            return Some(Rewrite::Unwrap(Some(NoteCode::LayoutFlattened)));
        }
        if (class("columnLayout") || class("cell") || class("innerCell"))
            && inside("contentLayout2")
        {
            return Some(Rewrite::Unwrap(None));
        }
        if class("table-wrap") || class("confluence-embedded-file-wrapper") {
            return Some(Rewrite::Unwrap(None));
        }
        if el.tag() == "span" {
            let style = el.attr("style").unwrap_or_default().to_ascii_lowercase();
            if style.contains("line-through") {
                return Some(Rewrite::Unwrap(Some(NoteCode::StrikethroughRemoved)));
            }
            if !style.is_empty() || el.attr("class").is_none() {
                return Some(Rewrite::Unwrap(None));
            }
        }
        if el.tag() == "div" && el.attr("class").is_none() {
            return Some(Rewrite::Unwrap(None));
        }
        None
    }
}

/// An image: an attachment or a web image, an emoticon, a file preview,
/// or Confluence's own icon.
fn image(el: &ElementView) -> Option<Rewrite> {
    if el.has_class("emoticon") {
        return Some(emoticon(el));
    }
    // A file preview (the view-file macro): its thumbnail isn't exported,
    // its link to the file stays.
    if el
        .parent()
        .is_some_and(|p| p.has_class("confluence-embedded-file"))
    {
        let a = el.parent()?;
        let name = a
            .attr("data-linked-resource-default-alias")
            .or_else(|| a.attr("aria-label"))
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "Attachment".to_string());
        return Some(Rewrite::Text {
            text: name,
            note: None,
        });
    }
    let src = el.attr("src").unwrap_or_default();
    if src.starts_with("images/icons/") {
        return Some(Rewrite::Remove(None));
    }
    if el.has_class("confluence-embedded-image") {
        return Some(Rewrite::Image {
            src,
            alt: el
                .attr("alt")
                .filter(|a| !a.trim().is_empty())
                .or_else(|| el.attr("data-linked-resource-default-alias")),
            width: el.attr("width").and_then(|w| html::parse_width(&w)),
            note: None,
        });
    }
    None
}

/// The label a callout with no title of its own gets.
fn callout_label(el: &ElementView) -> &'static str {
    if el.has_class("confluence-information-macro-note") {
        "Note"
    } else if el.has_class("confluence-information-macro-tip") {
        "Tip"
    } else if el.has_class("confluence-information-macro-warning") {
        "Warning"
    } else {
        "Info"
    }
}

/// The `brush` in `data-syntaxhighlighter-params` ("brush: java; gutter:
/// true").
fn brush(params: &str) -> Option<String> {
    params.split(';').find_map(|part| {
        let (name, value) = part.split_once(':')?;
        (name.trim() == "brush").then(|| value.trim().to_string())
    })
}

/// A code block language from Confluence's brush name. A code block with
/// no language exports as `java`, so that one can't be told apart.
pub fn language(brush: &str) -> Option<String> {
    let brush = brush.trim().to_ascii_lowercase();
    if brush.is_empty()
        || brush.len() > 40
        || !brush
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '#' | '+' | '-' | '_'))
    {
        return None;
    }
    let mapped = match brush.as_str() {
        "none" | "plain" | "text" | "txt" => return None,
        "js" | "jscript" => "javascript",
        "ts" => "typescript",
        "py" => "python",
        "c#" | "c-sharp" => "csharp",
        "sh" | "shell" => "bash",
        "ps" => "powershell",
        "yml" => "yaml",
        "c" | "c++" => "cpp",
        "rb" => "ruby",
        "pl" => "perl",
        "vb" => "vbnet",
        "as3" | "actionscript3" => "actionscript",
        "pascal" => "delphi",
        "patch" => "diff",
        "erl" => "erlang",
        "html" | "xhtml" | "xslt" => "xml",
        other => other,
    };
    Some(mapped.to_string())
}

/// An emoticon as its emoji: by the emoji id it names, else by its
/// Confluence name, else its alt text.
fn emoticon(el: &ElementView) -> Rewrite {
    if let Some(emoji) = el.attr("data-emoji-id").and_then(|id| emoji_from_id(&id)) {
        return Rewrite::Emoji(emoji);
    }
    let name = el
        .attr("data-emoticon-name")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if let Some(emoji) = emoticon_emoji(&name) {
        return Rewrite::Emoji(emoji.to_string());
    }
    let alt = el.attr("alt").unwrap_or_default();
    Rewrite::Text {
        text: alt.trim().to_string(),
        note: Some(NoteCode::EmojiAsText),
    }
}

/// Confluence's emoticon names.
pub fn emoticon_emoji(name: &str) -> Option<&'static str> {
    Some(match name {
        "smile" => "\u{1F642}",
        "sad" => "\u{1F641}",
        "cheeky" | "tongue" => "\u{1F61B}",
        "laugh" | "biggrin" => "\u{1F600}",
        "wink" => "\u{1F609}",
        "thumbs-up" => "\u{1F44D}",
        "thumbs-down" => "\u{1F44E}",
        "information" => "\u{2139}\u{FE0F}",
        "tick" => "\u{2705}",
        "cross" => "\u{274C}",
        "warning" => "\u{26A0}\u{FE0F}",
        "plus" => "\u{2795}",
        "minus" => "\u{2796}",
        "question" => "\u{2753}",
        "light-on" | "light-off" => "\u{1F4A1}",
        "yellow-star" | "red-star" | "green-star" | "blue-star" | "star" => "\u{2B50}",
        "heart" => "\u{2764}\u{FE0F}",
        "broken-heart" => "\u{1F494}",
        _ => return None,
    })
}

/// An emoji from the code points Confluence names it by (`1f600`,
/// `1f468-200d-1f4bb`), when they are an emoji's.
pub fn emoji_from_id(id: &str) -> Option<String> {
    let parts: Vec<&str> = id.trim().split('-').collect();
    if parts.is_empty() || parts.len() > 12 {
        return None;
    }
    let mut out = String::new();
    let mut pictographic = false;
    for part in parts {
        if part.is_empty() || part.len() > 6 {
            return None;
        }
        let code = u32::from_str_radix(part, 16).ok()?;
        let c = char::from_u32(code)?;
        let joiner =
            matches!(code, 0x200D | 0xFE0F | 0x20E3) || (0xE0020..=0xE007F).contains(&code);
        let keycap_base = c.is_ascii_digit() || c == '#' || c == '*';
        let emoji = code >= 0x1F000
            || (0x2100..=0x2BFF).contains(&code)
            || code == 0x00A9
            || code == 0x00AE;
        if !(joiner || keycap_base || emoji) {
            return None;
        }
        pictographic |= emoji;
        out.push(c);
    }
    pictographic.then_some(out)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};
    use std::path::Path;

    use serde_json::{json, Value};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    use super::*;
    use crate::services::docs_markdown::encode::{decode, encode_with_client_id};
    use crate::services::docs_markdown::import_tests::destinations;
    use crate::services::docs_markdown::ir::{blocks_text, Block};
    use crate::services::docs_markdown::links::{NoWorkspace, Placeholder};
    use crate::services::docs_markdown::tree::{self, Built, ImportOptions};
    use crate::services::docs_markdown::{normalise, DefaultResolver};
    use crate::services::uploaded_archive::{ArchiveOptions, SafeArchive};

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/docs_import");

    /// A fixture folder's files by archive key.
    fn fixture(name: &str) -> Vec<(String, Vec<u8>)> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
            for entry in std::fs::read_dir(dir).expect("fixture dir") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    walk(&path, base, out);
                } else {
                    let key = path
                        .strip_prefix(base)
                        .expect("under base")
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.push((key, std::fs::read(&path).expect("reads")));
                }
            }
        }
        let base = Path::new(FIXTURES).join(name);
        let mut out = Vec::new();
        walk(&base, &base, &mut out);
        out.sort();
        out
    }

    fn file<'a>(files: &'a mut [(String, Vec<u8>)], key: &str) -> &'a mut Vec<u8> {
        &mut files
            .iter_mut()
            .find(|(k, _)| k == key)
            .unwrap_or_else(|| panic!("no {key}"))
            .1
    }

    /// A zip as Confluence writes one: a `/` folder entry, the folders,
    /// then the files.
    fn confluence_zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
        let options = SimpleFileOptions::default();
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        w.add_directory("/", options).unwrap();
        let mut folders: Vec<String> = files
            .iter()
            .flat_map(|(k, _)| {
                let parts: Vec<&str> = k.split('/').collect();
                (1..parts.len())
                    .map(|n| parts[..n].join("/"))
                    .collect::<Vec<_>>()
            })
            .collect();
        folders.sort();
        folders.dedup();
        for folder in folders {
            w.add_directory(format!("{folder}/"), options).unwrap();
        }
        for (key, bytes) in files {
            w.start_file(key.as_str(), options).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn zip_of_zips(parts: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in parts {
            w.start_file(*name, SimpleFileOptions::default()).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn build_zip(bytes: Vec<u8>, options: &ImportOptions) -> Result<Built, Refused> {
        let mut archive =
            SafeArchive::open(Cursor::new(bytes), &ArchiveOptions::documentation()).expect("opens");
        tree::build(&mut archive, options, &NoWorkspace)
    }

    fn options() -> ImportOptions {
        ImportOptions {
            archive_name: Some("Confluence-space-export-021440-2.html.zip".into()),
            ..ImportOptions::default()
        }
    }

    fn built(name: &str) -> Built {
        build_zip(confluence_zip(&fixture(name)), &options()).expect("builds")
    }

    fn outline(built: &Built) -> Vec<String> {
        built
            .plan
            .pages
            .iter()
            .map(|page| {
                let mut depth = 0;
                let mut parent = page.parent;
                while let Some(p) = parent {
                    depth += 1;
                    parent = built.plan.pages[p].parent;
                }
                format!("{}{}", "  ".repeat(depth), page.title)
            })
            .collect()
    }

    fn index_of(built: &Built, title: &str) -> usize {
        built
            .plan
            .pages
            .iter()
            .position(|p| p.title == title)
            .unwrap_or_else(|| panic!("no page {title}: {:?}", outline(built)))
    }

    fn page_href(built: &Built, title: &str) -> String {
        Placeholder::Page(index_of(built, title)).to_href()
    }

    fn asset(key: &str) -> String {
        Placeholder::Asset(key.to_string()).to_href()
    }

    fn text_of(built: &Built, title: &str) -> Vec<String> {
        blocks_text(&built.documents[index_of(built, title)].blocks)
    }

    fn notes_of<'a>(built: &'a Built, title: &str) -> &'a Notes {
        &built.plan.pages[index_of(built, title)].notes
    }

    #[test]
    fn a_data_center_export_reads_into_its_tree() {
        let built = built("confluence-dc");
        assert_eq!(built.plan.profile, ProfileKind::Confluence);
        assert_eq!(built.plan.detected, None);
        assert_eq!(built.plan.root, "QA");
        assert_eq!(
            outline(&built),
            [
                "QA Knowledge Base Home",
                "Alpha Last Created",
                "Callouts and Panels",
                "Code Blocks",
                "Links",
                "Getting Started",
                "  FAQ",
                "  Install Guide",
                "    Deep Nested Page",
                "Images and Attachments",
                "Includes",
                "Layouts and Tables",
                "Macros Misc",
                "  Jira Issues",
                "Q&A: What's new? / \u{dc}n\u{ef}code \u{2713}",
                "Formatting",
                "Top Level Sibling",
            ]
        );
        // The homepage leads, its children beside it, in the space's order.
        let orders: Vec<(u32, Option<usize>)> = built
            .plan
            .pages
            .iter()
            .filter(|p| p.parent.is_none())
            .map(|p| (p.order, p.collection))
            .collect();
        assert_eq!(orders.len(), 13);
        assert!(orders
            .iter()
            .enumerate()
            .all(|(i, (order, collection))| *order == i as u32 && *collection == Some(0)));
        let collection = &built.plan.collections[0];
        assert_eq!(collection.name, "QA Knowledge Base");
        assert_eq!(
            collection.description.as_deref(),
            Some("Synthetic space for the Nosdesk import fixtures.")
        );
        // Pages are found again by space and page id, whatever the file name.
        let install = &built.plan.pages[index_of(&built, "Install Guide")];
        assert_eq!(install.key, "confluence:QA:294922");
        assert_eq!(
            install.source.as_deref(),
            Some("QA/Install-Guide_294922.html")
        );
        let special =
            &built.plan.pages[index_of(&built, "Q&A: What's new? / \u{dc}n\u{ef}code \u{2713}")];
        assert_eq!(special.key, "confluence:QA:294933");
        // Chrome stays out: no breadcrumbs, title, metadata or comments.
        let faq = text_of(&built, "FAQ");
        assert_eq!(
            faq,
            ["Short answer: restart it.", "Questions", "Why? Because."]
        );
        assert!(built.plan.pages.iter().all(|p| p.error.is_none()));
    }

    #[test]
    fn links_in_every_confluence_form_reach_the_page() {
        let built = built("confluence-dc");
        let links = &built.documents[index_of(&built, "Links")];
        let install = page_href(&built, "Install Guide");
        let getting = page_href(&built, "Getting Started");
        let faq = page_href(&built, "FAQ");
        let special = page_href(&built, "Q&A: What's new? / \u{dc}n\u{ef}code \u{2713}");
        assert_eq!(
            destinations(links),
            [
                // Relative, with and without an anchor.
                install.clone(),
                install.clone(),
                // viewpage.action, absolute on the export's own host.
                install.clone(),
                getting,
                // /display/KEY/Title, with an anchor.
                install,
                faq,
                // Another host keeps its link, as does a page in a space
                // the import doesn't hold.
                "https://example.atlassian.net/wiki/spaces/QA/pages/294922/Install+Guide".into(),
                "https://confluence.example.com/spaces/OPS/pages/294918/Ops+Runbook".into(),
                "https://confluence.example.com/pages/viewpage.action?pageId=294918".into(),
                "https://example.com/docs".into(),
                "mailto:help@example.com".into(),
                special,
            ]
        );
        let text = text_of(&built, "Links").join("\n");
        assert!(
            text.contains("Missing: Page That Does Not Exist."),
            "{text}"
        );
        let notes = notes_of(&built, "Links");
        assert_eq!(
            notes.count(NoteCode::LinksUnresolved),
            1,
            "the page that doesn't exist"
        );
        // The children display macro's absolute links reach the pages too.
        let home = destinations(&built.documents[index_of(&built, "QA Knowledge Base Home")]);
        assert_eq!(
            home.len(),
            12,
            "eleven children and Getting Started: {home:?}"
        );
        assert!(
            home.iter().all(|h| h.starts_with("nosdesk-import:page/")),
            "{home:?}"
        );
    }

    #[test]
    fn images_and_attachments_come_from_the_archive() {
        let built = built("confluence-dc");
        let media = &built.documents[index_of(&built, "Images and Attachments")];
        assert_eq!(
            destinations(media),
            [
                asset("QA/attachments/294930/294945.png"),
                asset("QA/attachments/294930/294946.png"),
                asset("QA/attachments/294930/294947.jpg"),
                // Sniffed at the check, which refuses an SVG.
                asset("QA/attachments/294930/294949.svg"),
                "https://example.com/banner.png".to_string(),
                asset("QA/attachments/294922/294951.png"),
            ]
        );
        let page = &built.plan.pages[index_of(&built, "Images and Attachments")];
        // Listed by the names Confluence gives them.
        assert_eq!(
            page.links.attachments.iter().collect::<Vec<_>>(),
            ["inventory.csv", "vpn-setup-guide.pdf"]
        );
        assert!(page.links.missing_images.is_empty(), "{:?}", page.links);
        // The PDF linked twice (a link and the file preview), the CSV once.
        assert_eq!(page.notes.count(NoteCode::AttachmentsLeftOut), 3);
        assert_eq!(
            page.notes.count(NoteCode::MacrosRemoved),
            1,
            "the attachments macro"
        );
        let text = text_of(&built, "Images and Attachments").join("\n");
        assert!(text.contains("vpn-setup-guide.pdf"), "{text}");
        // Alt text, else the file's name; the width kept.
        let mut images = Vec::new();
        for block in &media.blocks {
            if let Block::Paragraph(inlines) = block {
                for inline in inlines {
                    if let Inline::Image(image) = inline {
                        images.push((image.alt.clone(), image.width));
                    }
                }
            }
        }
        assert_eq!(images[0], (Some("Network diagram".into()), None));
        assert_eq!(images[1], (Some("server-room.png".into()), Some(300)));
        // Leaving the page out leaves its attachments out.
        assert!(built
            .plan
            .left_out
            .iter()
            .all(|l| !l.path.contains("attachments")));
    }

    #[test]
    fn macros_read_without_stray_html() {
        let built = built("confluence-dc");
        for page in &built.plan.pages {
            assert_eq!(page.notes.count(NoteCode::HtmlAsText), 0, "{}", page.title);
            assert_eq!(page.notes.count(NoteCode::HtmlRemoved), 0, "{}", page.title);
        }

        let callouts = notes_of(&built, "Callouts and Panels");
        assert_eq!(callouts.count(NoteCode::CalloutsAsQuotes), 7);
        assert_eq!(
            text_of(&built, "Callouts and Panels"),
            [
                "Heads up",
                "Info with a title.",
                "Info",
                "Info without a title.",
                "Careful",
                "A note.",
                "Tip",
                "A tip without a title.",
                "Danger zone",
                "A warning with bold.",
                "Panel title",
                "Panel body.",
                "Note",
                "Panel without a title.",
            ]
        );

        let code = &built.documents[index_of(&built, "Code Blocks")];
        let languages: Vec<Option<&str>> = code
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::CodeBlock { language, .. } => Some(language.as_deref()),
                _ => None,
            })
            .collect();
        assert_eq!(
            languages,
            [
                Some("java"),
                Some("javascript"),
                Some("python"),
                Some("bash"),
                Some("sql"),
                Some("powershell"),
                Some("yaml"),
                Some("csharp"),
                // No language exports as Java.
                Some("java"),
                None,
            ]
        );
        let code_text = text_of(&built, "Code Blocks");
        assert_eq!(code_text[0], "Main.java");
        assert!(
            code_text.contains(&"  text <b>not bold</b>".to_string()),
            "{code_text:?}"
        );

        let misc = notes_of(&built, "Macros Misc");
        assert_eq!(misc.count(NoteCode::ExpandsShownOpen), 2);
        assert_eq!(misc.count(NoteCode::StatusAsText), 3);
        assert_eq!(misc.count(NoteCode::ChecklistsAsBullets), 1);
        assert_eq!(misc.count(NoteCode::MentionsAsText), 1);
        assert_eq!(misc.count(NoteCode::EmojiAsText), 10);
        // Table of contents, page tree, recently updated, contributors and
        // the attachments macro; children display stays.
        assert_eq!(misc.count(NoteCode::MacrosRemoved), 5);
        let text = text_of(&built, "Macros Misc");
        for line in [
            "Show details",
            "Hidden content.",
            "Click here to expand...",
            "State: DONE and IN PROGRESS and GREY",
            "\u{2610} Open task",
            "\u{2611} Done task bold",
            "Ask @Sam Example for help.",
            "Tick \u{2705} cross \u{274c} warning \u{26a0}\u{fe0f} info \u{2139}\u{fe0f} smile \u{1f642} sad \u{1f641} thumbs \u{1f44d} light \u{1f4a1} star \u{2b50} emoji \u{1f600}",
            "Jira Issues",
        ] {
            assert!(text.contains(&line.to_string()), "{line:?} in {text:?}");
        }
        assert!(
            !text.iter().any(|l| l.contains("Recently Updated")),
            "{text:?}"
        );

        let jira = notes_of(&built, "Jira Issues");
        assert_eq!(
            jira.count(NoteCode::MacrosRemoved),
            3,
            "unrendered Jira macros"
        );
        assert_eq!(
            text_of(&built, "Jira Issues"),
            [
                "Single issue: ",
                "Issue with server: ",
                "Plain link: ABC-125"
            ]
        );

        let layout = notes_of(&built, "Layouts and Tables");
        assert_eq!(layout.count(NoteCode::LayoutFlattened), 1);
        let tables = &built.documents[index_of(&built, "Layouts and Tables")];
        assert_eq!(
            tables
                .blocks
                .iter()
                .filter(|b| matches!(b, Block::Table(_)))
                .count(),
            2
        );
        let text = text_of(&built, "Layouts and Tables");
        assert_eq!(text[..2], ["Left column.", "Right column."]);
        assert!(
            text.contains(&"1 · First".to_string()),
            "numbering kept: {text:?}"
        );

        let includes = notes_of(&built, "Includes");
        assert_eq!(
            includes.count(NoteCode::MacrosRemoved),
            1,
            "the missing include"
        );
        assert_eq!(
            includes.count(NoteCode::CalloutsAsQuotes),
            1,
            "the excerpt's panel"
        );

        let formatting = notes_of(&built, "Formatting");
        assert_eq!(formatting.count(NoteCode::StrikethroughRemoved), 5);
        assert!(text_of(&built, "Formatting").contains(
            &"bold italic underline struck deleted sub sup mono red text line break".to_string()
        ));
    }

    #[test]
    fn a_zip_of_spaces_is_a_collection_each_linked_across() {
        let qa = confluence_zip(&fixture("confluence-dc"));
        let ops = confluence_zip(&fixture("confluence-dc-ops"));
        let built = build_zip(
            zip_of_zips(&[("QA.html.zip", qa), ("OPS.html.zip", ops)]),
            &options(),
        )
        .expect("builds");
        let names: Vec<&str> = built
            .plan
            .collections
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, ["Operations", "QA Knowledge Base"]);
        // The homepage's child sits beside it, in the space's collection.
        let runbook = index_of(&built, "Ops Runbook");
        let home = index_of(&built, "Operations Home");
        assert_eq!(built.plan.pages[home].collection, Some(0));
        assert_eq!(built.plan.pages[runbook].collection, Some(0));
        assert_eq!(built.plan.pages[runbook].parent, None);
        assert_eq!(built.plan.pages[runbook].order, 1);
        let links = destinations(&built.documents[index_of(&built, "Links")]);
        let to_runbook = page_href(&built, "Ops Runbook");
        assert_eq!(
            links.iter().filter(|h| **h == to_runbook).count(),
            2,
            "by /spaces/OPS/pages/ and by viewpage.action: {links:?}"
        );
        assert_eq!(built.plan.pages[runbook].key, "confluence:OPS:294918");
    }

    #[test]
    fn a_custom_export_without_the_homepage_keeps_its_tree() {
        let built = built("confluence-dc-partial");
        assert_eq!(outline(&built), ["Install Guide", "  Deep Nested Page"]);
        assert_eq!(
            destinations(&built.documents[index_of(&built, "Deep Nested Page")]),
            ["https://confluence.example.com/spaces/QA/pages/294921/Getting+Started"],
            "a page the export doesn't hold keeps its link to Confluence"
        );
    }

    #[test]
    fn pages_missing_from_the_index_go_under_their_breadcrumbs() {
        let mut files = fixture("confluence-dc");
        let index = file(&mut files, "QA/index.html");
        let text = String::from_utf8(index.clone()).unwrap();
        let text = text
            .replace(
                "<a href=\"Deep-Nested-Page_294923.html\">Deep Nested Page</a>",
                "",
            )
            .replace(
                "<a href=\"Top-Level-Sibling_294976.html\">Top Level Sibling</a>",
                "",
            );
        *index = text.into_bytes();
        let built = build_zip(confluence_zip(&files), &options()).expect("builds");
        let lines = outline(&built);
        assert!(
            lines.contains(&"    Deep Nested Page".to_string()),
            "{lines:?}"
        );
        assert_eq!(lines.last().map(String::as_str), Some("Top Level Sibling"));
        let deep = &built.plan.pages[index_of(&built, "Deep Nested Page")];
        assert_eq!(deep.parent, Some(index_of(&built, "Install Guide")));
        // Its title comes from the page itself.
        assert_eq!(deep.title, "Deep Nested Page");
    }

    #[test]
    fn an_xml_backup_or_pages_without_their_index_are_refused() {
        let err = build_zip(confluence_zip(&fixture("confluence-dc-xml")), &options()).unwrap_err();
        assert_eq!(err.code, "docs_import_confluence_xml");
        assert_eq!(
            err.message,
            "This is a Confluence XML export. Export the space as HTML instead."
        );

        let files: Vec<(String, Vec<u8>)> = fixture("confluence-dc")
            .into_iter()
            .filter(|(k, _)| k != "QA/index.html")
            .collect();
        let err = build_zip(confluence_zip(&files), &options()).unwrap_err();
        assert_eq!(err.code, "docs_import_confluence_no_index");

        // Forced on something else, the source says what it needs.
        let markdown = vec![("notes/a.md".to_string(), b"# A".to_vec())];
        let err = build_zip(
            confluence_zip(&markdown),
            &ImportOptions {
                profile: Some(ProfileKind::Confluence),
                ..options()
            },
        )
        .unwrap_err();
        assert_eq!(err.code, "docs_import_confluence_no_index");
    }

    #[test]
    fn a_stray_markdown_file_doesnt_hide_the_export() {
        let mut files = fixture("confluence-dc");
        files.push((
            "QA/attachments/294930/294999.md".into(),
            b"# Notes".to_vec(),
        ));
        let built = build_zip(confluence_zip(&files), &options()).expect("builds");
        assert_eq!(built.plan.profile, ProfileKind::Confluence);
        assert_eq!(built.plan.pages.len(), 17);
    }

    #[test]
    fn hostile_markup_is_read_as_content_only() {
        let index = r#"<html><head><title>EVIL (Evil)</title></head><body>
            <div id="main-content" class="pageSection"><table><tr><th>Key</th><td>EVIL</td></tr></table></div>
            <div class="pageSection"><div class="pageSectionHeader"><h2 class="pageSectionTitle">Available Pages:</h2></div>
            <ul><li><a href="Page_1.html">Page</a></li><li><a href="../outside.html">Outside</a></li>
            <li><a href="https://example.com/x.html">Web</a></li></ul></div>
            <div id="footer"><p>Document generated by Confluence</p></div></body></html>"#;
        let page = r#"<html><body><div id="main-content" class="wiki-content">
            <script>alert(1)</script><p onclick="x()">Hello <a href="javascript:alert(1)">bad</a>
            <a href="nosdesk-import:page/0">forged</a> <img src="x.png" onerror="alert(1)"/>
            <span class="status-macro" style="background:url(javascript:alert(1))">OK</span></p>
            <img class="emoticon" data-emoji-id="202e-61" alt="(rtl)"/>
            </div></body></html>"#;
        let files = vec![
            ("EVIL/index.html".to_string(), index.as_bytes().to_vec()),
            ("EVIL/Page_1.html".to_string(), page.as_bytes().to_vec()),
        ];
        let built = build_zip(confluence_zip(&files), &options()).expect("builds");
        assert_eq!(outline(&built), ["Page"]);
        let document = &built.documents[0];
        assert_eq!(destinations(document), Vec::<String>::new());
        let text = blocks_text(&document.blocks).join("\n");
        assert!(!text.contains("alert"), "{text}");
        assert!(text.contains("Hello bad forged"), "{text}");
        assert!(text.contains("OK"), "the status's text: {text}");
        assert!(
            text.contains("(rtl)"),
            "an emoji id that isn't one keeps its alt: {text}"
        );
        let notes = &built.plan.pages[0].notes;
        assert_eq!(notes.count(NoteCode::HtmlRemoved), 1, "the script");
        assert!(notes.count(NoteCode::LinksDropped) >= 2, "{notes:?}");
    }

    #[test]
    fn markdown_with_an_index_html_is_not_confluence() {
        let mut read = |_: &str| Some("<html><body><h1>Docs</h1></body></html>".to_string());
        let files = vec!["site/index.html".to_string(), "site/about.html".to_string()];
        assert!(detect(&files, &mut read).is_none());

        // A space exported with no pages: its index alone isn't a missing
        // index.
        let index = String::from_utf8(
            fixture("confluence-dc")
                .into_iter()
                .find(|(k, _)| k == "QA/index.html")
                .unwrap()
                .1,
        )
        .unwrap();
        let mut read = |_: &str| Some(index.clone());
        let files = vec![
            "QA/index.html".to_string(),
            "QA/styles/site.css".to_string(),
        ];
        assert!(detect(&files, &mut read).is_none());
    }

    #[test]
    fn brushes_map_to_languages() {
        for (brush, language_name) in [
            ("java", Some("java")),
            ("js", Some("javascript")),
            ("py", Some("python")),
            ("c#", Some("csharp")),
            ("csharp", Some("csharp")),
            ("sh", Some("bash")),
            ("bash", Some("bash")),
            ("yml", Some("yaml")),
            ("ps", Some("powershell")),
            ("powershell", Some("powershell")),
            ("sql", Some("sql")),
            ("xml", Some("xml")),
            ("html", Some("xml")),
            ("none", None),
            ("", None),
            ("a b", None),
            ("<script>", None),
        ] {
            assert_eq!(language(brush).as_deref(), language_name, "{brush}");
        }
        assert_eq!(brush("brush: c#; gutter: false").as_deref(), Some("c#"));
        assert_eq!(brush("gutter: false"), None);
    }

    #[test]
    fn emoticons_become_emoji() {
        assert_eq!(emoticon_emoji("tick"), Some("\u{2705}"));
        assert_eq!(emoticon_emoji("cross"), Some("\u{274c}"));
        assert_eq!(emoticon_emoji("warning"), Some("\u{26a0}\u{fe0f}"));
        assert_eq!(emoticon_emoji("information"), Some("\u{2139}\u{fe0f}"));
        assert_eq!(emoticon_emoji("smile"), Some("\u{1f642}"));
        assert_eq!(emoticon_emoji("nope"), None);
        assert_eq!(emoji_from_id("1f600").as_deref(), Some("\u{1f600}"));
        assert_eq!(
            emoji_from_id("1f468-200d-1f4bb").as_deref(),
            Some("\u{1f468}\u{200d}\u{1f4bb}")
        );
        assert_eq!(
            emoji_from_id("31-fe0f-20e3"),
            None,
            "a keycap alone has no pictograph"
        );
        assert_eq!(emoji_from_id("202e"), None);
        assert_eq!(emoji_from_id("61"), None);
        assert_eq!(emoji_from_id("zz"), None);
        assert_eq!(emoji_from_id(&"1f600-".repeat(20)), None);
    }

    #[test]
    fn confluence_urls_name_pages_and_attachments() {
        let mut source = Confluence::new(String::new());
        source.by_id.insert("12".into(), "KB/Setup_12.html".into());
        source
            .by_title
            .insert(("kb".into(), "vpn & wi-fi".into()), "KB/VPN_13.html".into());
        source.attachments.insert(
            ("12".into(), "guide v2.pdf".into()),
            "KB/attachments/12/40.pdf".into(),
        );
        source.hosts.insert("wiki.example.com:8090".into());
        for (href, target) in [
            (
                "https://wiki.example.com:8090/pages/viewpage.action?pageId=12",
                Some("KB/Setup_12.html"),
            ),
            (
                "/confluence/pages/viewpage.action?pageId=12#x",
                Some("KB/Setup_12.html"),
            ),
            (
                "/pages/viewpage.action?spaceKey=KB&title=VPN+%26+Wi-Fi",
                Some("KB/VPN_13.html"),
            ),
            ("/display/KB/VPN+%26+Wi-Fi", Some("KB/VPN_13.html")),
            ("/spaces/KB/pages/12/Setup", Some("KB/Setup_12.html")),
            ("/wiki/spaces/KB/pages/12", Some("KB/Setup_12.html")),
            (
                "/download/attachments/12/guide%20v2.pdf?version=1&api=v2",
                Some("KB/attachments/12/40.pdf"),
            ),
            (
                "/download/thumbnails/12/guide%20v2.pdf",
                Some("KB/attachments/12/40.pdf"),
            ),
            (
                "https://other.example.com/pages/viewpage.action?pageId=12",
                None,
            ),
            ("/pages/viewpage.action?pageId=99", None),
            ("/display/~sam", None),
            ("//wiki.example.com/display/KB/VPN", None),
            ("Setup_12.html", None),
            ("javascript:alert(1)", None),
        ] {
            assert_eq!(source.archive_target(href).as_deref(), target, "{href}");
        }
    }

    #[test]
    fn index_titles_and_file_names() {
        let index = read_index(
            &String::from_utf8(
                fixture("confluence-dc")
                    .into_iter()
                    .find(|(k, _)| k == "QA/index.html")
                    .unwrap()
                    .1,
            )
            .unwrap(),
            "QA",
        );
        assert_eq!(index.key, "QA");
        assert_eq!(index.name, "QA Knowledge Base");
        assert_eq!(index.pages.len(), 2);
        assert!(index.pages[0].home);
        assert_eq!(index.pages[0].children.len(), 11);
        assert_eq!(
            title_from_file("QA/Install-Guide_294922.html"),
            "Install Guide"
        );
        assert_eq!(title_from_file("QA/294933.html"), "294933");
        assert_eq!(
            decode_entities("Q&amp;A &#39;x&#x27; &bogus; &"),
            "Q&A 'x' &bogus; &"
        );
        assert_eq!(page_id("QA/Version_2_12345.html", ""), Some("12345".into()));
        assert_eq!(page_id("QA/Notes.html", ""), None);
    }

    // Golden fixtures: each page of the export read through the dialect,
    // encoded, and checked against the editor schema by
    // `docsMarkdownFixtures.spec.ts`. Regenerate with
    // `UPDATE_DOCS_MARKDOWN_FIXTURES=1 cargo test docs_markdown`.

    const GOLDEN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/docs_markdown");

    const GOLDEN_PAGES: [(&str, &str); 9] = [
        ("confluence-callouts", "QA/Callouts-and-Panels_294925.html"),
        ("confluence-code", "QA/Code-Blocks_294926.html"),
        ("confluence-macros", "QA/Macros-Misc_294927.html"),
        ("confluence-jira", "QA/Jira-Issues_294928.html"),
        ("confluence-layouts", "QA/Layouts-and-Tables_294929.html"),
        ("confluence-includes", "QA/Includes_294931.html"),
        ("confluence-media", "QA/Images-and-Attachments_294930.html"),
        ("confluence-links", "QA/Links_294932.html"),
        ("confluence-formatting", "QA/Formatting_294977.html"),
    ];

    #[test]
    fn confluence_golden_fixtures_are_current() {
        use base64::Engine as _;
        let engine = base64::engine::general_purpose::STANDARD;
        let regenerate = std::env::var_os("UPDATE_DOCS_MARKDOWN_FIXTURES").is_some();
        let files = fixture("confluence-dc");
        let mut stale = Vec::new();
        for (name, key) in GOLDEN_PAGES {
            let html = files
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, b)| String::from_utf8(b.clone()).expect("utf-8"))
                .expect("fixture page");
            let mut notes = Notes::default();
            let document = html::read_document(
                &html,
                &CONTENT,
                &ConfluenceDialect,
                &DefaultResolver,
                &mut notes,
            );
            let document = normalise::normalise(document, &DefaultResolver, &mut notes);
            let fresh = encode_with_client_id(&document, 1);
            let path = format!("{GOLDEN_DIR}/{name}.yjs.json");
            let stored = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok());
            let current = stored
                .as_ref()
                .and_then(|v| v["update"].as_str())
                .and_then(|b| engine.decode(b).ok());
            let matches = stored.as_ref().is_some_and(|v| v["notes"] == json!(notes))
                && current
                    .as_deref()
                    .is_some_and(|c| decode::to_json(c) == decode::to_json(&fresh));
            if matches {
                continue;
            }
            if regenerate {
                let body = json!({
                    "source": name,
                    "notes": notes,
                    "update": engine.encode(&fresh),
                });
                let text = serde_json::to_string_pretty(&body).expect("serialises") + "\n";
                std::fs::write(&path, text).expect("fixture writes");
            } else {
                stale.push(name);
            }
        }
        assert!(
            stale.is_empty(),
            "stale Confluence fixtures {stale:?}; run UPDATE_DOCS_MARKDOWN_FIXTURES=1 cargo test docs_markdown"
        );
    }
}
