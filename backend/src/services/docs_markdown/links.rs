//! Where an imported page's links, images and embeds point.
//!
//! A link inside the import resolves to a [`Placeholder`] naming the target
//! page by its index in the plan; the commit swaps it for the page's id
//! (`/documentation/{id}`) or uuid once the page exists. Images in the
//! archive resolve to a placeholder naming their archive key, stored by the
//! commit. Placeholders are only ever produced here, from a resolved
//! target: a placeholder written in a source file is an unknown scheme and
//! is dropped like any other.
//!
//! Resolution, per [`PageResolver`]:
//!
//! - Relative paths, with or without `.md`, URL-decoded, `./` and `../`, a
//!   folder to its page; `/path` from the source root, then the archive's.
//!   A file that is neither a page nor an image is an attachment, left out.
//! - Wikilinks by path, then by file name anywhere (the nearest folder wins;
//!   more than one match is reported as ambiguous), then by front matter
//!   `aliases`. `#heading` and `^block` parts are dropped.
//! - Notion URLs by the 32-hex id their pages carry; Nosdesk
//!   `/documentation/{slug}` by the slugs in the import.
//! - Mentions, ticket links and embedded pages outside the import survive
//!   only when the archive came from this workspace and the target exists
//!   ([`Workspace`]).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use super::ir::{Block, Document, Inline, ListItem};
use super::normalise::{Resolver, WikiEmbed, WIKILINK_PREFIX};
use super::{DefaultResolver, NoteCode, Notes};

/// The scheme of the destinations the commit rewrites.
pub const PLACEHOLDER_SCHEME: &str = "nosdesk-import:";

/// A destination the commit fills in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Placeholder {
    /// A page in the plan, by index.
    Page(usize),
    /// A file in the archive, by key.
    Asset(String),
}

impl Placeholder {
    pub fn to_href(&self) -> String {
        match self {
            Placeholder::Page(index) => format!("{PLACEHOLDER_SCHEME}page/{index}"),
            Placeholder::Asset(key) => format!("{PLACEHOLDER_SCHEME}asset/{key}"),
        }
    }

    pub fn parse(href: &str) -> Option<Self> {
        let rest = href.strip_prefix(PLACEHOLDER_SCHEME)?;
        if let Some(index) = rest.strip_prefix("page/") {
            return index.parse().ok().map(Placeholder::Page);
        }
        rest.strip_prefix("asset/")
            .filter(|key| !key.is_empty())
            .map(|key| Placeholder::Asset(key.to_string()))
    }
}

/// What the target workspace holds, for the spans and links that point
/// outside the import. Implemented over the database by the import job; the
/// defaults hold nothing.
pub trait Workspace {
    /// Whether an archive exported from `workspace_uuid` came from this
    /// workspace.
    fn is_this_workspace(&self, _workspace_uuid: &str) -> bool {
        false
    }
    fn user_exists(&self, _uuid: &str) -> bool {
        false
    }
    fn ticket_exists(&self, _ticket_id: &str) -> bool {
        false
    }
    /// A documentation page by uuid.
    fn page_exists(&self, _uuid: &str) -> bool {
        false
    }
    /// A documentation page by the id or slug a `/documentation/...` link
    /// names.
    fn page_reference_exists(&self, _reference: &str) -> bool {
        false
    }
    /// An image stored in this workspace, by its `/api/files/...` source.
    fn image_exists(&self, _src: &str) -> bool {
        false
    }
}

/// A workspace that holds nothing: every span outside the import is text.
pub struct NoWorkspace;

impl Workspace for NoWorkspace {}

/// Where the editor's images are served from.
const COLLAB_IMAGE_PREFIX: &str = "/api/files/collab/doc/";

/// Every way an import's pages and files can be named.
#[derive(Debug, Default)]
pub struct LinkIndex {
    /// The source root inside the archive (`docs`), or empty.
    pub root: String,
    /// Every readable file, lowercased key to key.
    files: HashMap<String, String>,
    /// Page file keys, lowercased, to page.
    pages: HashMap<String, usize>,
    /// Folder keys, lowercased, to the page that stands for the folder.
    folders: HashMap<String, usize>,
    /// File stems and cleaned names, lowercased, to pages.
    names: HashMap<String, Vec<usize>>,
    aliases: HashMap<String, Vec<usize>>,
    /// File names, lowercased, to file keys: for `![[image.png]]`.
    file_names: HashMap<String, Vec<String>>,
    notion_ids: HashMap<String, usize>,
    slugs: HashMap<String, usize>,
    uuids: HashMap<String, usize>,
    /// Each page's title and the folder its file sits in.
    titles: Vec<String>,
    dirs: Vec<String>,
}

impl LinkIndex {
    pub fn new(root: &str) -> Self {
        LinkIndex {
            root: root.to_string(),
            ..LinkIndex::default()
        }
    }

    pub fn add_file(&mut self, key: &str) {
        self.files.insert(key.to_lowercase(), key.to_string());
        let name = key.rsplit('/').next().unwrap_or(key).to_lowercase();
        self.file_names
            .entry(name)
            .or_default()
            .push(key.to_string());
    }

    /// Register page `index`, titled `title`, from `file` (its key) or for
    /// `folder` (its key), or both.
    pub fn add_page(
        &mut self,
        index: usize,
        title: &str,
        file: Option<&str>,
        folder: Option<&str>,
    ) {
        if self.titles.len() <= index {
            self.titles.resize(index + 1, String::new());
            self.dirs.resize(index + 1, String::new());
        }
        self.titles[index] = title.to_string();
        let anchor = file.or(folder).unwrap_or("");
        self.dirs[index] = parent(anchor).to_string();
        if let Some(file) = file {
            self.pages.insert(file.to_lowercase(), index);
            let stem = file_stem(file);
            self.add_name(stem, index);
            if let Some(id) = notion_id(stem) {
                self.notion_ids.insert(id.to_string(), index);
                self.add_name(strip_notion_id(stem), index);
            }
        }
        if let Some(folder) = folder {
            self.folders.insert(folder.to_lowercase(), index);
            let name = folder.rsplit('/').next().unwrap_or(folder);
            if let Some(id) = notion_id(name) {
                self.notion_ids.entry(id.to_string()).or_insert(index);
            }
            if file.is_none() {
                self.add_name(name, index);
            }
        }
    }

    fn add_name(&mut self, name: &str, index: usize) {
        let entry = self.names.entry(name.to_lowercase()).or_default();
        if !entry.contains(&index) {
            entry.push(index);
        }
    }

    pub fn add_alias(&mut self, alias: &str, index: usize) {
        let alias = alias.trim().to_lowercase();
        if !alias.is_empty() {
            self.aliases.entry(alias).or_default().push(index);
        }
    }

    pub fn add_slug(&mut self, slug: &str, index: usize) {
        self.slugs.entry(slug.to_lowercase()).or_insert(index);
    }

    pub fn add_uuid(&mut self, uuid: &str, index: usize) {
        self.uuids.entry(uuid.to_lowercase()).or_insert(index);
    }

    pub fn title(&self, index: usize) -> &str {
        self.titles.get(index).map(String::as_str).unwrap_or("")
    }

    /// The page with Nosdesk uuid `uuid`, if the import holds it.
    pub fn page_by_uuid(&self, uuid: &str) -> Option<usize> {
        self.uuids.get(&uuid.to_lowercase()).copied()
    }

    /// What the archive path `path` (already joined and normalised) names.
    fn target_at(&self, path: &str) -> Target {
        let lower = path.trim_end_matches('/').to_lowercase();
        if let Some(&page) = self.pages.get(&lower) {
            return Target::Page(page);
        }
        for ext in [".md", ".mdx", ".markdown"] {
            if let Some(&page) = self.pages.get(&format!("{lower}{ext}")) {
                return Target::Page(page);
            }
        }
        if let Some(&page) = self.folders.get(&lower) {
            return Target::Page(page);
        }
        if let Some(key) = self.files.get(&lower) {
            return Target::File(key.clone());
        }
        Target::Missing
    }

    /// A path as a link from the folder `dir` writes it: relative to `dir`,
    /// or from the source root when it starts with `/` (then from the
    /// archive's root).
    fn resolve_path(&self, dir: &str, path: &str) -> Target {
        if let Some(absolute) = path.strip_prefix('/') {
            for base in [self.root.as_str(), ""] {
                if let Some(joined) = join_normalised(base, absolute) {
                    let target = self.target_at(&joined);
                    if target != Target::Missing {
                        return target;
                    }
                }
            }
            return Target::Missing;
        }
        join_normalised(dir, path)
            .map(|joined| self.target_at(&joined))
            .unwrap_or(Target::Missing)
    }

    /// Pages a wikilink names, nearest to `dir` first.
    fn by_name(&self, name: &str, dir: &str) -> Vec<usize> {
        let lower = name.to_lowercase();
        let mut found = self.names.get(&lower).cloned().unwrap_or_default();
        if found.is_empty() {
            found = self.aliases.get(&lower).cloned().unwrap_or_default();
        }
        found.sort_by_key(|&page| {
            let page_dir = self.dirs.get(page).map(String::as_str).unwrap_or("");
            (distance(dir, page_dir), page)
        });
        found.dedup();
        found
    }
}

/// How many folders apart two folders are.
fn distance(from: &str, to: &str) -> usize {
    let a: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let b: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    (a.len() - common) + (b.len() - common)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Page(usize),
    File(String),
    Missing,
}

/// What became of one destination.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    /// Kept or rewritten to this.
    Kept(String),
    /// Not a destination a page can hold (`javascript:`, a bare fragment).
    Dropped,
    /// Names nothing in the import.
    Unresolved,
    /// Names a file that isn't a page or an image.
    Attachment,
}

/// What a page's links and images did, for Review.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkReport {
    /// Links rewritten to pages in the import, counted once per target.
    pub rewritten: u32,
    /// Destinations that name nothing in the import, as written.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub unresolved: BTreeSet<String>,
    /// Names more than one page share, with the pages (nearest first; the
    /// link goes to the first).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub ambiguous: BTreeMap<String, Vec<usize>>,
    /// Files linked that aren't imported (PDFs, spreadsheets).
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub attachments: BTreeSet<String>,
    /// Images the page shows that the import can't find.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub missing_images: BTreeSet<String>,
    /// Archive keys of the images the page shows.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub images: BTreeSet<String>,
}

/// One page's resolver: the index, where the page sits, and what the
/// target workspace holds.
pub struct PageResolver<'a> {
    pub index: &'a LinkIndex,
    /// The folder the page's file sits in.
    pub dir: String,
    pub workspace: &'a dyn Workspace,
    /// The archive was exported from this workspace.
    pub same_workspace: bool,
    report: RefCell<LinkReport>,
    notes: RefCell<Notes>,
    links: RefCell<HashMap<String, Outcome>>,
    images: RefCell<HashMap<String, Outcome>>,
}

impl<'a> PageResolver<'a> {
    pub fn new(
        index: &'a LinkIndex,
        dir: &str,
        workspace: &'a dyn Workspace,
        same_workspace: bool,
    ) -> Self {
        PageResolver {
            index,
            dir: dir.to_string(),
            workspace,
            same_workspace,
            report: RefCell::new(LinkReport::default()),
            notes: RefCell::new(Notes::default()),
            links: RefCell::new(HashMap::new()),
            images: RefCell::new(HashMap::new()),
        }
    }

    /// The report and the notes the resolver counted itself.
    pub fn finish(self) -> (LinkReport, Notes) {
        (self.report.into_inner(), self.notes.into_inner())
    }

    fn page_href(&self, page: usize) -> String {
        Placeholder::Page(page).to_href()
    }

    fn link_outcome(&self, href: &str) -> Outcome {
        if let Some(outcome) = self.links.borrow().get(href) {
            return outcome.clone();
        }
        let outcome = self.classify_link(href);
        match &outcome {
            Outcome::Kept(kept) if kept.starts_with(PLACEHOLDER_SCHEME) => {
                self.report.borrow_mut().rewritten += 1;
            }
            Outcome::Unresolved => {
                self.report.borrow_mut().unresolved.insert(href_label(href));
            }
            Outcome::Attachment => {
                self.report
                    .borrow_mut()
                    .attachments
                    .insert(href_label(href));
            }
            _ => {}
        }
        self.links
            .borrow_mut()
            .insert(href.to_string(), outcome.clone());
        outcome
    }

    fn classify_link(&self, href: &str) -> Outcome {
        let href = href.trim();
        if let Some(target) = href.strip_prefix(WIKILINK_PREFIX) {
            return self.wikilink(target);
        }
        if let Some(scheme) = scheme(href) {
            if scheme == "http" || scheme == "https" {
                if let Some(page) = self.notion_url(href) {
                    return Outcome::Kept(self.page_href(page));
                }
            }
            return match DefaultResolver.link(href) {
                Some(kept) => Outcome::Kept(kept),
                None => Outcome::Dropped,
            };
        }
        if href.starts_with("//") {
            return Outcome::Dropped;
        }
        let path = strip_fragment(href);
        if path.is_empty() {
            // A fragment on this page.
            return Outcome::Dropped;
        }
        if let Some(reference) = path.strip_prefix("/documentation/") {
            let reference = reference.trim_end_matches('/');
            if let Some(&page) = self.index.slugs.get(&reference.to_lowercase()) {
                return Outcome::Kept(self.page_href(page));
            }
            if self.same_workspace && self.workspace.page_reference_exists(reference) {
                return Outcome::Kept(format!("/documentation/{reference}"));
            }
            return Outcome::Unresolved;
        }
        match self.index.resolve_path(&self.dir, &percent_decode(path)) {
            Target::Page(page) => Outcome::Kept(self.page_href(page)),
            Target::File(_) => Outcome::Attachment,
            Target::Missing => Outcome::Unresolved,
        }
    }

    /// A Notion page URL: the 32-hex id at the end of its path.
    fn notion_url(&self, href: &str) -> Option<usize> {
        let host_and_path = href.split_once("://")?.1;
        let (host, path) = host_and_path.split_once('/')?;
        let host = host.to_ascii_lowercase();
        if !(host.ends_with("notion.so") || host.ends_with("notion.site")) {
            return None;
        }
        let path = path.split(['?', '#']).next().unwrap_or("");
        let last = path.rsplit('/').next().unwrap_or("");
        let id: String = last
            .chars()
            .rev()
            .filter(|c| *c != '-')
            .take(32)
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        let id = id.to_ascii_lowercase();
        (id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| self.index.notion_ids.get(&id).copied())
            .flatten()
    }

    /// `[[target]]`: by path, then by name, then by alias.
    fn wikilink(&self, target: &str) -> Outcome {
        let name = wikilink_name(target);
        if name.is_empty() {
            return Outcome::Dropped;
        }
        match self.wiki_target(name) {
            Target::Page(page) => Outcome::Kept(self.page_href(page)),
            Target::File(_) => Outcome::Attachment,
            Target::Missing => Outcome::Unresolved,
        }
    }

    fn wiki_target(&self, name: &str) -> Target {
        if name.contains('/') {
            for base in [self.index.root.as_str(), "", self.dir.as_str()] {
                if let Some(joined) = join_normalised(base, name) {
                    let target = self.index.target_at(&joined);
                    if target != Target::Missing {
                        return target;
                    }
                }
            }
        }
        let stem = name
            .strip_suffix(".md")
            .or_else(|| name.strip_suffix(".mdx"))
            .unwrap_or(name);
        let leaf = stem.rsplit('/').next().unwrap_or(stem);
        let pages = self.index.by_name(leaf, &self.dir);
        if let Some(&first) = pages.first() {
            if pages.len() > 1 {
                let mut report = self.report.borrow_mut();
                if !report.ambiguous.contains_key(name) {
                    report.ambiguous.insert(name.to_string(), pages.clone());
                    self.notes.borrow_mut().add(NoteCode::LinksAmbiguous);
                }
            }
            return Target::Page(first);
        }
        let file_name = name.rsplit('/').next().unwrap_or(name).to_lowercase();
        match self.index.file_names.get(&file_name) {
            Some(keys) => {
                let mut keys = keys.clone();
                keys.sort_by_key(|k| (distance(&self.dir, parent(k)), k.clone()));
                Target::File(keys[0].clone())
            }
            None => Target::Missing,
        }
    }

    fn image_outcome(&self, src: &str) -> Outcome {
        if let Some(outcome) = self.images.borrow().get(src) {
            return outcome.clone();
        }
        let outcome = self.classify_image(src);
        {
            let mut report = self.report.borrow_mut();
            match &outcome {
                Outcome::Kept(kept) => {
                    if let Some(Placeholder::Asset(key)) = Placeholder::parse(kept) {
                        report.images.insert(key);
                    }
                }
                Outcome::Unresolved | Outcome::Attachment => {
                    report.missing_images.insert(href_label(src));
                }
                Outcome::Dropped => {}
            }
        }
        self.images
            .borrow_mut()
            .insert(src.to_string(), outcome.clone());
        outcome
    }

    fn classify_image(&self, src: &str) -> Outcome {
        let src = src.trim();
        let target = if let Some(target) = src.strip_prefix(WIKILINK_PREFIX) {
            self.wiki_target(wikilink_name(target))
        } else if let Some(scheme) = scheme(src) {
            return match scheme.as_str() {
                "http" | "https" => match DefaultResolver.image(src) {
                    Some(kept) => Outcome::Kept(kept),
                    None => Outcome::Dropped,
                },
                "data" if is_data_image(src) => Outcome::Kept(src.to_string()),
                _ => Outcome::Dropped,
            };
        } else if src.starts_with(COLLAB_IMAGE_PREFIX) {
            return if self.workspace.image_exists(src) {
                Outcome::Kept(src.to_string())
            } else {
                Outcome::Unresolved
            };
        } else if src.starts_with("//") {
            return Outcome::Dropped;
        } else {
            let path = strip_fragment(src);
            if path.is_empty() {
                return Outcome::Dropped;
            }
            self.index.resolve_path(&self.dir, &percent_decode(path))
        };
        match target {
            Target::File(key) if is_image_name(&key) => {
                Outcome::Kept(Placeholder::Asset(key).to_href())
            }
            Target::File(_) => Outcome::Attachment,
            Target::Page(_) | Target::Missing => Outcome::Unresolved,
        }
    }
}

impl Resolver for PageResolver<'_> {
    fn link(&self, href: &str) -> Option<String> {
        match self.link_outcome(href) {
            Outcome::Kept(kept) => Some(kept),
            _ => None,
        }
    }

    fn image(&self, src: &str) -> Option<String> {
        match self.image_outcome(src) {
            Outcome::Kept(kept) => Some(kept),
            _ => None,
        }
    }

    fn dropped_link_note(&self, href: &str) -> NoteCode {
        match self.link_outcome(href) {
            Outcome::Unresolved => NoteCode::LinksUnresolved,
            Outcome::Attachment => NoteCode::AttachmentsLeftOut,
            _ => NoteCode::LinksDropped,
        }
    }

    fn mention(&self, uuid: &str) -> bool {
        self.same_workspace && self.workspace.user_exists(uuid)
    }

    fn ticket(&self, ticket_id: &str, href: &str) -> Option<String> {
        let id = ticket_id.trim();
        if !self.same_workspace || id.is_empty() || !self.workspace.ticket_exists(id) {
            return None;
        }
        let local = href.starts_with('/') && !href.starts_with("//");
        if local {
            return Some(href.to_string());
        }
        let id: i32 = id.parse().ok()?;
        Some(crate::utils::ticket_link::ticket_route_by_id(id))
    }

    fn embed(&self, document_uuid: &str) -> Option<String> {
        let uuid = document_uuid.trim();
        if uuid.is_empty() {
            return None;
        }
        if let Some(page) = self.index.page_by_uuid(uuid) {
            self.report.borrow_mut().rewritten += 1;
            return Some(self.page_href(page));
        }
        (self.same_workspace && self.workspace.page_exists(uuid)).then(|| uuid.to_string())
    }

    fn wiki_embed(&self, target: &str) -> WikiEmbed {
        let name = wikilink_name(target);
        if name.is_empty() {
            return WikiEmbed::Missing;
        }
        if is_image_name(name) {
            return WikiEmbed::Image;
        }
        match self.wiki_target(name) {
            Target::Page(page) => {
                self.report.borrow_mut().rewritten += 1;
                WikiEmbed::Page {
                    document_uuid: self.page_href(page),
                    document_title: self.index.title(page).to_string(),
                }
            }
            Target::File(key) if is_image_name(&key) => WikiEmbed::Image,
            Target::File(_) => {
                self.report
                    .borrow_mut()
                    .attachments
                    .insert(name.to_string());
                self.notes.borrow_mut().add(NoteCode::AttachmentsLeftOut);
                WikiEmbed::Missing
            }
            Target::Missing => {
                self.report.borrow_mut().unresolved.insert(name.to_string());
                self.notes.borrow_mut().add(NoteCode::LinksUnresolved);
                WikiEmbed::Missing
            }
        }
    }
}

/// A wikilink target without its `#heading` or `^block` part.
fn wikilink_name(target: &str) -> &str {
    let end = target.find(['#', '^']).unwrap_or(target.len());
    target[..end].trim()
}

/// A destination as Review lists it: as written, without the wikilink
/// marker.
fn href_label(href: &str) -> String {
    href.strip_prefix(WIKILINK_PREFIX)
        .map(|t| format!("[[{t}]]"))
        .unwrap_or_else(|| href.trim().to_string())
}

fn strip_fragment(href: &str) -> &str {
    let end = href.find(['#', '?']).unwrap_or(href.len());
    &href[..end]
}

/// The lowercased scheme, when `url` has one. A drive letter (`C:`) is not
/// a scheme a link can use, and is caught as an unknown one.
fn scheme(url: &str) -> Option<String> {
    let colon = url.find(':')?;
    let scheme = &url[..colon];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic()
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}

fn is_data_image(src: &str) -> bool {
    let lower = src.get(..32).unwrap_or(src).to_ascii_lowercase();
    ["png", "jpeg", "jpg", "gif", "webp"]
        .iter()
        .any(|t| lower.starts_with(&format!("data:image/{t};base64,")))
}

/// A file name the image route can serve (SVG is refused at upload).
pub fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".avif"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// `%XX` sequences decoded; invalid UTF-8 replaced.
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The folder part of an archive key.
pub fn parent(key: &str) -> &str {
    key.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

/// A file name without its extension.
pub fn file_stem(key: &str) -> &str {
    let name = key.rsplit('/').next().unwrap_or(key);
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    }
}

/// `path` joined to `dir`, with `.` and `..` resolved. `None` when it
/// climbs above the archive root.
pub fn join_normalised(dir: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for segment in path.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// The 32-hex id a Notion export appends to a name (`Setup 1a2b...`).
pub fn notion_id(name: &str) -> Option<&str> {
    let (_, id) = name.rsplit_once(' ')?;
    (id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

/// A name without a Notion id.
pub fn strip_notion_id(name: &str) -> &str {
    match notion_id(name) {
        Some(id) => name[..name.len() - id.len()].trim_end(),
        None => name,
    }
}

/// Where a placeholder sits: a link takes a page's route, an image a stored
/// file's source, an embedded page the page's uuid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Link,
    Image,
    Embed,
}

/// What [`fill_placeholders`] asks of its caller.
pub type Fill<'a> = &'a dyn Fn(&Placeholder, Slot) -> Option<String>;

/// Replace every placeholder in a converted document. `fill` returns the
/// final destination for the slot the placeholder sits in, or `None` to drop
/// it (a link keeps its text, an image its alt text, an embedded page its
/// title).
pub fn fill_placeholders(document: &mut Document, fill: Fill<'_>) {
    fill_blocks(&mut document.blocks, fill);
}

fn fill_blocks(blocks: &mut [Block], fill: Fill<'_>) {
    for block in blocks.iter_mut() {
        match block {
            Block::Paragraph(inlines)
            | Block::Heading {
                content: inlines, ..
            } => fill_inlines(inlines, fill),
            Block::Blockquote(inner) => fill_blocks(inner, fill),
            Block::BulletList(items) | Block::OrderedList { items, .. } => fill_items(items, fill),
            Block::Table(table) => {
                for cell in table.rows.iter_mut().flatten() {
                    fill_blocks(&mut cell.blocks, fill);
                }
            }
            Block::EmbeddedDocument {
                document_uuid,
                document_title,
            } => {
                if let Some(placeholder) = Placeholder::parse(document_uuid) {
                    match fill(&placeholder, Slot::Embed) {
                        Some(uuid) => *document_uuid = uuid,
                        None => {
                            *block = Block::Paragraph(vec![Inline::text(document_title.clone())]);
                        }
                    }
                }
            }
            Block::CodeBlock { .. } | Block::HorizontalRule => {}
        }
    }
}

fn fill_items(items: &mut [ListItem], fill: Fill<'_>) {
    for item in items {
        fill_blocks(&mut item.blocks, fill);
    }
}

fn fill_inlines(inlines: &mut Vec<Inline>, fill: Fill<'_>) {
    for inline in inlines.iter_mut() {
        match inline {
            Inline::Text(text) => {
                let filled = text
                    .marks
                    .link
                    .as_ref()
                    .and_then(|link| Placeholder::parse(&link.href))
                    .map(|p| fill(&p, Slot::Link));
                if let Some(filled) = filled {
                    match filled {
                        Some(href) => {
                            if let Some(link) = text.marks.link.as_mut() {
                                link.href = href;
                            }
                        }
                        None => text.marks.link = None,
                    }
                }
            }
            Inline::Image(image) => {
                if let Some(placeholder) = Placeholder::parse(&image.src) {
                    match fill(&placeholder, Slot::Image) {
                        Some(src) => image.src = src,
                        None => {
                            let alt = image.alt.clone().unwrap_or_default();
                            *inline = Inline::text(alt);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    inlines.retain(|i| !matches!(i, Inline::Text(t) if t.text.is_empty()));
    super::normalise::merge_adjacent_text(inlines);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_join_and_never_climb_out() {
        assert_eq!(join_normalised("a/b", "../c.md").as_deref(), Some("a/c.md"));
        assert_eq!(join_normalised("a", "./b/./c").as_deref(), Some("a/b/c"));
        assert_eq!(join_normalised("a", "../../x"), None);
        assert_eq!(join_normalised("", "a\\b").as_deref(), Some("a/b"));
    }

    #[test]
    fn percent_sequences_decode() {
        assert_eq!(percent_decode("My%20Page%201a.md"), "My Page 1a.md");
        assert_eq!(percent_decode("caf%C3%A9"), "café");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn notion_ids_are_read_from_names() {
        let id = "1a2b3c4d5e6f70819a2b3c4d5e6f7081";
        assert_eq!(notion_id(&format!("Setup {id}")), Some(id));
        assert_eq!(strip_notion_id(&format!("VPN setup {id}")), "VPN setup");
        assert_eq!(notion_id("Setup 1234"), None);
    }

    #[test]
    fn placeholders_round_trip_and_fill() {
        for p in [Placeholder::Page(7), Placeholder::Asset("a b/c.png".into())] {
            assert_eq!(Placeholder::parse(&p.to_href()), Some(p));
        }
        assert_eq!(Placeholder::parse("nosdesk-import:page/x"), None);
        assert_eq!(Placeholder::parse("https://x"), None);
    }

    #[test]
    fn a_page_fills_by_slot() {
        use super::super::ir::{Link, Marks};
        let page = Placeholder::Page(3).to_href();
        let mut document = Document {
            blocks: vec![
                Block::Paragraph(vec![Inline::marked(
                    "Setup",
                    Marks {
                        link: Some(Link {
                            href: page.clone(),
                            title: None,
                        }),
                        ..Marks::default()
                    },
                )]),
                Block::EmbeddedDocument {
                    document_uuid: page,
                    document_title: "Setup".into(),
                },
            ],
        };
        fill_placeholders(&mut document, &|p, slot| match (p, slot) {
            (Placeholder::Page(3), Slot::Link) => Some("/documentation/41".into()),
            (Placeholder::Page(3), Slot::Embed) => Some("0199-uuid".into()),
            _ => None,
        });
        let Block::Paragraph(inlines) = &document.blocks[0] else {
            panic!("a paragraph");
        };
        let Inline::Text(text) = &inlines[0] else {
            panic!("text");
        };
        assert_eq!(text.marks.link.as_ref().unwrap().href, "/documentation/41");
        assert_eq!(
            document.blocks[1],
            Block::EmbeddedDocument {
                document_uuid: "0199-uuid".into(),
                document_title: "Setup".into(),
            }
        );
    }
}
