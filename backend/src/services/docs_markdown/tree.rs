//! An archive of Markdown as a plan of pages, and each page as a document.
//!
//! [`build`] reads an uploaded archive under a source profile and returns:
//!
//! - the [`Plan`]: every page with its source, title, parent, order among
//!   its siblings, collection, status, icon, slug and the front matter it
//!   used; the collections to create; and per page the degrade notes and a
//!   link report, for Review. Nothing is written.
//! - the converted [`Document`] of each page, its links to other pages and
//!   its archive images as [`Placeholder`](super::links::Placeholder)s the
//!   commit fills in once the pages exist.
//!
//! Titles: front matter `title`, then an H1 that opens the file, then a
//! SUMMARY, nav or `_category_` label, then the file name cleaned (ids and
//! number prefixes stripped, `kebab-case` humanised). An opening H1 equal to
//! the title is removed from the body.
//!
//! Order among siblings: a nav's order, then explicit order keys (front
//! matter `weight`, `nav_order`, `order`, `sidebar_position`, `_category_`
//! `position`, the manifest's order), then, for Notion, the order the parent
//! page links its children, then number prefixes, then a natural sort of
//! the title.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::front_matter::{self, Fields};
use super::ir::{Block, Document, Inline, Table, TableCell};
use super::links::{
    file_stem, join_normalised, parent, percent_decode, strip_notion_id, LinkIndex, LinkReport,
    PageResolver, Workspace,
};
use super::parse::{self, ParseOptions};
use super::profiles::{
    self, is_markdown, Detection, Listing, Manifest, ManifestPage, NavItem, ProfileKind, Rules,
};
use super::{normalise, FrontMatter, NoteCode, Notes};
use crate::services::imports::refusal::Refused;
use crate::services::uploaded_archive::{SafeArchive, SkipReason};

/// Pages one import may hold.
pub const MAX_PAGES: usize = 2_000;
/// Largest Markdown file read.
pub const MAX_MARKDOWN_BYTES: u64 = 2 * 1024 * 1024;
/// Largest config file or database CSV read.
const MAX_CONFIG_BYTES: u64 = 10 * 1024 * 1024;
/// Longest title kept.
pub const MAX_TITLE_CHARS: usize = 255;
/// Rows of a Notion database shown on its page.
pub const DATABASE_ROWS: usize = 200;
/// Longest icon kept.
const MAX_ICON_CHARS: usize = 50;

/// Where the pages go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionMode {
    /// One new collection, named after the archive.
    New,
    /// One new collection per top-level folder.
    PerTopLevelFolder,
    /// An existing collection the job names; the plan creates none.
    Existing,
    /// As the source says (a Nosdesk export's collections); otherwise
    /// [`CollectionMode::New`].
    AsSource,
}

/// The status imported pages get.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusMode {
    #[default]
    Drafts,
    Published,
    /// `draft: true`, `status: draft`, `published: false` and the manifest's
    /// status are honoured.
    AsFiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageStatus {
    Draft,
    Published,
    Archived,
}

/// The options an admin sets at Pages.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportOptions {
    /// Forces a profile; detection's otherwise.
    pub profile: Option<ProfileKind>,
    /// The profile's default when unset.
    pub collections: Option<CollectionMode>,
    /// The new collection's name; the source's site name or the archive's
    /// name otherwise.
    pub collection_name: Option<String>,
    /// The uploaded file's name, for the default collection name.
    pub archive_name: Option<String>,
    pub status: StatusMode,
    /// Globs of archive paths left out (`**/archive/**`).
    pub leave_out: Vec<String>,
    /// Pages the admin unticked, left out with everything under them.
    pub exclude: ExcludedPages,
}

/// Pages left out by what they are rather than where they sit in the
/// tree, so a tree read under other leave-out patterns finds them still: a
/// page goes when its file or its folder is listed, or, for a page with
/// neither (a nav heading), its titles from the top down.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExcludedPages {
    pub files: BTreeSet<String>,
    pub folders: BTreeSet<String>,
    pub trails: BTreeSet<Vec<String>>,
}

impl ExcludedPages {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.folders.is_empty() && self.trails.is_empty()
    }

    /// Leave out the page with this file, folder and trail of titles.
    pub fn add(&mut self, file: Option<&str>, folder: Option<&str>, trail: &[String]) {
        if let Some(file) = file {
            self.files.insert(file.to_string());
        }
        if let Some(folder) = folder {
            self.folders.insert(folder.to_string());
        }
        if file.is_none() && folder.is_none() {
            self.trails.insert(trail.to_vec());
        }
    }

    pub fn matches(&self, file: Option<&str>, folder: Option<&str>, trail: &[String]) -> bool {
        match (file, folder) {
            (None, None) => self.trails.contains(trail),
            _ => {
                file.is_some_and(|f| self.files.contains(f))
                    || folder.is_some_and(|f| self.folders.contains(f))
            }
        }
    }
}

/// What a page stands for, for the tree's icon and pills.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageKind {
    /// A Markdown file.
    #[default]
    Page,
    /// A folder with no page file of its own: a page with no content.
    Section,
    /// A Notion database, its rows shown as a table.
    Database,
    /// A row of a Notion database, as its own page.
    DatabaseRow,
}

/// A collection the import creates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedCollection {
    pub name: String,
    pub slug: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
    /// The source's uuid for it (a Nosdesk export).
    pub source_uuid: Option<String>,
}

/// One page the import creates.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlannedPage {
    pub index: usize,
    /// The Markdown file, by archive key.
    pub source: Option<String>,
    /// The folder the page stands for, by archive key.
    pub folder: Option<String>,
    pub title: String,
    pub parent: Option<usize>,
    /// Position among its siblings, from 0.
    pub order: u32,
    /// The collection a top-level page goes in, by index.
    pub collection: Option<usize>,
    pub kind: PageKind,
    pub status: PageStatus,
    pub icon: Option<String>,
    /// A slug to keep when it is free.
    pub slug: Option<String>,
    /// A Nosdesk uuid, for spotting a page imported before.
    pub uuid: Option<String>,
    /// Front matter keys this page's import used.
    pub front_matter_used: Vec<String>,
    pub notes: Notes,
    pub links: LinkReport,
    /// Why the page's file couldn't be read; it is imported empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A file the plan leaves out, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LeftOut {
    pub path: String,
    pub reason: &'static str,
}

/// What an import will do.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plan {
    pub profile: ProfileKind,
    /// The folder the source sits in inside the archive, if any.
    pub root: String,
    /// What detection said, when an override replaced it.
    pub detected: Option<ProfileKind>,
    pub collections: Vec<PlannedCollection>,
    pub pages: Vec<PlannedPage>,
    /// Pages standing for a folder with no page file of its own.
    pub section_pages: u32,
    /// Front matter keys not imported, with the number of pages using each.
    pub front_matter_not_imported: BTreeMap<String, u32>,
    pub left_out: Vec<LeftOut>,
    /// Every page's notes added up.
    pub notes: Notes,
}

/// A plan and each page's converted document, by page index.
#[derive(Debug, Clone)]
pub struct Built {
    pub plan: Plan,
    pub documents: Vec<Document>,
}

fn refused_no_markdown() -> Refused {
    Refused::new(
        "docs_import_no_markdown",
        "There are no Markdown files in this zip.",
    )
}

fn refused_too_many(count: usize) -> Refused {
    Refused::new(
        "docs_import_too_many_pages",
        format!(
            "The import has {count} pages. The limit is {MAX_PAGES}. Export one part at a time."
        ),
    )
    .arg("count", count)
    .arg("max", MAX_PAGES)
}

/// Read `archive` into a plan and documents. Reads every Markdown file once;
/// nothing is written anywhere.
pub fn build(
    archive: &mut SafeArchive,
    options: &ImportOptions,
    workspace: &dyn Workspace,
) -> Result<Built, Refused> {
    let leave_out: Vec<&str> = options.leave_out.iter().map(String::as_str).collect();
    let mut left_out = Vec::new();
    let files: Vec<String> = archive
        .entries()
        .iter()
        .map(|e| e.key.clone())
        .filter(|key| {
            let keep = !leave_out.iter().any(|g| glob_match(g, key));
            if !keep {
                left_out.push(LeftOut {
                    path: key.clone(),
                    reason: "left_out",
                });
            }
            keep
        })
        .collect();
    let ignored: Vec<String> = archive
        .skipped()
        .iter()
        .filter(|s| s.reason == SkipReason::Ignored)
        .map(|s| s.key.clone())
        .collect();
    let markdown: Vec<String> = files
        .iter()
        .filter(|k| is_markdown(k) && !in_tool_folder(k))
        .cloned()
        .collect();
    if markdown.len() > MAX_PAGES {
        return Err(refused_too_many(markdown.len()));
    }

    let mut texts: HashMap<String, String> = HashMap::new();
    let mut errors: HashMap<String, String> = HashMap::new();
    let mut lossy: HashSet<String> = HashSet::new();
    for key in &markdown {
        match archive.read_text(key, MAX_MARKDOWN_BYTES) {
            Ok(text) => {
                if text.lossy {
                    lossy.insert(key.clone());
                }
                texts.insert(key.clone(), text.text);
            }
            Err(e) => {
                errors.insert(key.clone(), e.to_string());
            }
        }
    }

    let mut read_small = |key: &str| {
        archive
            .read_text(key, MAX_CONFIG_BYTES)
            .ok()
            .map(|t| t.text)
    };
    let detection = profiles::detect(&mut Listing {
        files: &files,
        ignored: &ignored,
        read: &mut read_small,
        markdown: &texts,
    });
    let (kind, detected, base) = match options.profile {
        // A forced profile reads what detection would refuse, and says what
        // detection thought, for "Change".
        Some(kind) => {
            let base = detection
                .as_ref()
                .map(|d| d.base.clone())
                .unwrap_or_else(|_| profiles::wrapper_folder(&files));
            let detected = detection.ok().map(|d| d.kind).filter(|d| *d != kind);
            (kind, detected, base)
        }
        None => {
            let Detection { kind, base } = detection?;
            (kind, None, base)
        }
    };
    if markdown.is_empty() {
        return Err(refused_no_markdown());
    }
    let rules = kind.rules();

    let mut source = Source {
        kind,
        rules,
        base,
        files: &files,
        texts: &texts,
        read: &mut read_small,
        nodes: vec![Node::root()],
        left_out: &mut left_out,
        manifest: None,
        site_name: None,
    };
    source.lay_out();
    let Source {
        nodes,
        manifest,
        site_name,
        base,
        ..
    } = source;
    let root = base.clone();

    Planner {
        kind,
        detected,
        rules,
        options,
        workspace,
        root,
        files: &files,
        texts: &texts,
        errors: &errors,
        lossy: &lossy,
        manifest,
        site_name,
        left_out,
        read: &mut read_small,
    }
    .plan(nodes)
}

/// Files under a tool's own folder (`.gitbook/`), which hold assets and
/// includes rather than pages.
fn in_tool_folder(key: &str) -> bool {
    key.split('/')
        .rev()
        .skip(1)
        .any(|segment| segment.starts_with('.'))
}

// The source's tree.

#[derive(Debug, Clone, Default)]
struct Node {
    file: Option<String>,
    folder: Option<String>,
    /// A nav, SUMMARY or `_category_` label.
    label: Option<String>,
    nav_index: Option<usize>,
    /// An explicit order from the source (a `_category_` position, the
    /// manifest's order).
    order: Option<f64>,
    /// A root `index.md` or `README.md`, ordered first among its siblings.
    first: bool,
    /// A folder or a heading group: it may become a collection.
    group: bool,
    /// A Notion database's CSV, shown on the page.
    csv: Option<String>,
    manifest: Option<ManifestPage>,
    parent: Option<usize>,
    children: Vec<usize>,
}

impl Node {
    fn root() -> Self {
        Node::default()
    }
}

struct Source<'a> {
    kind: ProfileKind,
    rules: Rules,
    base: String,
    files: &'a [String],
    texts: &'a HashMap<String, String>,
    read: &'a mut dyn FnMut(&str) -> Option<String>,
    nodes: Vec<Node>,
    left_out: &'a mut Vec<LeftOut>,
    manifest: Option<Manifest>,
    site_name: Option<String>,
}

fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else if name.is_empty() {
        base.to_string()
    } else {
        format!("{base}/{name}")
    }
}

fn under(root: &str, key: &str) -> bool {
    root.is_empty() || key.starts_with(&format!("{root}/"))
}

impl Source<'_> {
    fn add(&mut self, node: Node, parent: usize) -> usize {
        let index = self.nodes.len();
        self.nodes.push(Node {
            parent: Some(parent),
            ..node
        });
        self.nodes[parent].children.push(index);
        index
    }

    fn markdown_under(&self, root: &str) -> Vec<String> {
        let mut keys: Vec<String> = self
            .files
            .iter()
            .filter(|k| is_markdown(k) && !in_tool_folder(k) && under(root, k))
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    fn lay_out(&mut self) {
        match self.kind {
            ProfileKind::NosdeskV2 => self.manifest_layout(),
            ProfileKind::NosdeskV1 => {
                let root = join(&self.base, "documentation");
                let root = if self.markdown_under(&root).is_empty() {
                    self.base.clone()
                } else {
                    root
                };
                self.base = root.clone();
                let keys = self.markdown_under(&root);
                self.folder_layout(&root, &keys, 0);
            }
            ProfileKind::Gitbook => self.gitbook_layout(),
            ProfileKind::Mkdocs => self.mkdocs_layout(),
            ProfileKind::Docusaurus => {
                let docs = join(&self.base, "docs");
                if !self.markdown_under(&docs).is_empty() {
                    self.base = docs;
                }
                let root = self.base.clone();
                let keys = self.markdown_under(&root);
                self.folder_layout(&root, &keys, 0);
                self.apply_categories();
            }
            ProfileKind::Notion
            | ProfileKind::Obsidian
            | ProfileKind::Outline
            | ProfileKind::Plain => {
                let root = self.base.clone();
                let keys = self.markdown_under(&root);
                self.folder_layout(&root, &keys, 0);
            }
        }
    }

    /// Folders become parent pages. A folder's page is its `index.md`,
    /// `README.md` or `_index.md`, a same-name file inside it (when the
    /// profile says), or a same-name file beside it; otherwise it is a
    /// section page with no content.
    fn folder_layout(&mut self, root: &str, keys: &[String], under_node: usize) {
        let mut folders: Vec<String> = Vec::new();
        for key in keys {
            let mut dir = parent(key);
            while !dir.is_empty() && dir != root && under(root, dir) {
                folders.push(dir.to_string());
                dir = parent(dir);
            }
        }
        folders.sort();
        folders.dedup();
        let find = |dir: &str, name: &str| -> Option<String> {
            ["md", "mdx", "markdown"].iter().find_map(|ext| {
                let wanted = join(dir, &format!("{name}.{ext}")).to_lowercase();
                keys.iter().find(|k| k.to_lowercase() == wanted).cloned()
            })
        };

        let mut claimed: HashMap<String, String> = HashMap::new();
        for folder in &folders {
            let name = folder.rsplit('/').next().unwrap_or(folder);
            let page = ["index", "README", "_index"]
                .iter()
                .find_map(|n| find(folder, n))
                .or_else(|| {
                    self.rules
                        .folder_note_inside
                        .then(|| find(folder, name))
                        .flatten()
                })
                .or_else(|| find(parent(folder), name));
            if let Some(page) = page.filter(|p| !claimed.contains_key(p)) {
                claimed.insert(page, folder.clone());
            }
        }
        let folder_page: HashMap<&str, &str> = claimed
            .iter()
            .map(|(file, folder)| (folder.as_str(), file.as_str()))
            .collect();

        // Nodes for folders, outermost first, then files.
        let mut node_of: HashMap<String, usize> = HashMap::new();
        node_of.insert(root.to_string(), under_node);
        for folder in &folders {
            let parent_node = node_of.get(parent(folder)).copied().unwrap_or(under_node);
            let csv = if folder_page.contains_key(folder.as_str()) {
                None
            } else {
                self.database_csv(folder)
            };
            let node = self.add(
                Node {
                    file: folder_page.get(folder.as_str()).map(|f| f.to_string()),
                    folder: Some(folder.clone()),
                    group: true,
                    csv,
                    ..Node::default()
                },
                parent_node,
            );
            node_of.insert(folder.clone(), node);
        }
        for key in keys {
            if claimed.contains_key(key) {
                continue;
            }
            let dir = parent(key);
            let parent_node = node_of.get(dir).copied().unwrap_or(under_node);
            let stem = file_stem(key).to_ascii_lowercase();
            let first = dir == root && matches!(stem.as_str(), "index" | "readme" | "_index");
            self.add(
                Node {
                    file: Some(key.clone()),
                    first,
                    ..Node::default()
                },
                parent_node,
            );
        }
    }

    /// A Notion database: `Name.csv` (or `Name_all.csv`, all its rows)
    /// beside the folder of its rows' pages.
    fn database_csv(&self, folder: &str) -> Option<String> {
        if !self.rules.notion_ids {
            return None;
        }
        let all = format!("{folder}_all.csv").to_lowercase();
        let view = format!("{folder}.csv").to_lowercase();
        self.files
            .iter()
            .find(|k| k.to_lowercase() == all)
            .or_else(|| self.files.iter().find(|k| k.to_lowercase() == view))
            .cloned()
    }

    /// Docusaurus `_category_` files name and order their folders.
    fn apply_categories(&mut self) {
        for i in 0..self.nodes.len() {
            let Some(folder) = self.nodes[i].folder.clone() else {
                continue;
            };
            let key = ["_category_.json", "_category_.yml", "_category_.yaml"]
                .iter()
                .map(|n| join(&folder, n))
                .find(|k| self.files.contains(k));
            let Some(key) = key else {
                continue;
            };
            if let Some(text) = (self.read)(&key) {
                let category = profiles::read_category(&key, &text);
                self.nodes[i].label = category.label;
                self.nodes[i].order = category.position;
            }
        }
    }

    fn gitbook_layout(&mut self) {
        let config_text = (self.read)(&join(&self.base, ".gitbook.yaml"));
        let config = profiles::read_gitbook(config_text.as_deref());
        let root = join(&self.base, &config.root);
        self.base = root.clone();
        let summary_key = join(&root, &config.summary);
        let nav = self
            .texts
            .get(&summary_key)
            .map(|t| profiles::read_summary(t))
            .unwrap_or_default();
        let mut claimed: HashSet<String> = HashSet::new();
        claimed.insert(summary_key.clone());
        let readme = join(&root, &config.readme);
        if self.texts.contains_key(&readme) && !nav_mentions(&nav, &root, &config.readme) {
            let node = self.add(
                Node {
                    file: Some(readme.clone()),
                    first: true,
                    ..Node::default()
                },
                0,
            );
            self.nodes[node].nav_index = Some(0);
            claimed.insert(readme);
        }
        self.nav_layout(&nav, &root, 0, &mut claimed, 1);
        self.leftovers(&root, &claimed);
    }

    fn mkdocs_layout(&mut self) {
        let key = ["mkdocs.yml", "mkdocs.yaml"]
            .iter()
            .map(|n| join(&self.base, n))
            .find(|k| self.files.contains(k));
        let config = key
            .and_then(|k| (self.read)(&k))
            .map(|t| profiles::read_mkdocs(&t))
            .unwrap_or_default();
        let docs_dir = if config.docs_dir.is_empty() {
            "docs".to_string()
        } else {
            config.docs_dir.clone()
        };
        let root = join(&self.base, &docs_dir);
        let root = if self.markdown_under(&root).is_empty() {
            self.base.clone()
        } else {
            root
        };
        self.base = root.clone();
        self.site_name = config.site_name.clone();
        match &config.nav {
            Some(nav) => {
                let mut claimed = HashSet::new();
                self.nav_layout(nav, &root, 0, &mut claimed, 0);
                self.leftovers(&root, &claimed);
            }
            None => {
                let keys = self.markdown_under(&root);
                self.folder_layout(&root, &keys, 0);
            }
        }
    }

    /// Pages from a nav, in its order. A section with no page of its own
    /// becomes a section page.
    fn nav_layout(
        &mut self,
        items: &[NavItem],
        dir: &str,
        parent_node: usize,
        claimed: &mut HashSet<String>,
        offset: usize,
    ) {
        for (i, item) in items.iter().enumerate() {
            let file = item
                .path
                .as_deref()
                .filter(|p| !p.contains("://"))
                .and_then(|p| {
                    let path = percent_decode(p.split(['#', '?']).next().unwrap_or(""));
                    let joined = join_normalised(dir, &path)?;
                    let candidates = [
                        joined.clone(),
                        join(&joined, "index.md"),
                        join(&joined, "README.md"),
                        format!("{joined}.md"),
                    ];
                    candidates
                        .into_iter()
                        .find(|c| self.texts.contains_key(c) && !claimed.contains(c))
                });
            if file.is_none() && item.children.is_empty() {
                continue;
            }
            if let Some(file) = &file {
                claimed.insert(file.clone());
            }
            let node = self.add(
                Node {
                    group: file.is_none() || item.group,
                    file,
                    label: item.title.clone(),
                    nav_index: Some(offset + i),
                    ..Node::default()
                },
                parent_node,
            );
            self.nav_layout(&item.children, dir, node, claimed, 0);
        }
    }

    /// Files a nav doesn't list, as top-level pages after the nav's.
    fn leftovers(&mut self, root: &str, claimed: &HashSet<String>) {
        for key in self.markdown_under(root) {
            if !claimed.contains(&key) {
                self.add(
                    Node {
                        file: Some(key),
                        ..Node::default()
                    },
                    0,
                );
            }
        }
    }

    /// A Nosdesk export's tree, from its manifest.
    fn manifest_layout(&mut self) {
        let key = join(&self.base, profiles::MANIFEST);
        let manifest = (self.read)(&key)
            .and_then(|t| profiles::read_manifest(&t))
            .unwrap_or_default();
        let mut node_of_uuid: HashMap<String, usize> = HashMap::new();
        let mut listed: HashSet<String> = HashSet::new();
        let mut deleted: HashMap<String, Option<String>> = HashMap::new();
        let mut pending: Vec<(usize, Option<String>)> = Vec::new();
        for page in &manifest.pages {
            let file = join(&self.base, &page.path);
            listed.insert(file.clone());
            if page.status.as_deref() == Some("deleted") {
                if let Some(uuid) = &page.uuid {
                    deleted.insert(uuid.to_lowercase(), page.parent.clone());
                }
                self.left_out.push(LeftOut {
                    path: file,
                    reason: "deleted",
                });
                continue;
            }
            let node = self.nodes.len();
            self.nodes.push(Node {
                file: Some(file),
                order: page.order.map(f64::from),
                manifest: Some(page.clone()),
                ..Node::default()
            });
            if let Some(uuid) = &page.uuid {
                node_of_uuid.insert(uuid.to_lowercase(), node);
            }
            pending.push((node, page.parent.clone()));
        }
        for (node, parent_uuid) in pending {
            // A parent in the trash hands its children to its own parent.
            let mut parent_uuid = parent_uuid.map(|p| p.to_lowercase());
            let mut hops = 0;
            while let Some(p) = parent_uuid.clone() {
                match deleted.get(&p) {
                    Some(grand) if hops < 64 => {
                        parent_uuid = grand.clone().map(|g| g.to_lowercase());
                        hops += 1;
                    }
                    _ => break,
                }
            }
            let parent_node = parent_uuid
                .and_then(|p| node_of_uuid.get(&p).copied())
                .filter(|p| *p != node)
                .unwrap_or(0);
            self.nodes[node].parent = Some(parent_node);
            self.nodes[parent_node].children.push(node);
        }
        // A parent loop leaves pages reachable from nowhere: lift them.
        let reachable = reachable(&self.nodes);
        for node in 1..self.nodes.len() {
            if !reachable.contains(&node) {
                if let Some(old) = self.nodes[node].parent {
                    self.nodes[old].children.retain(|c| *c != node);
                }
                self.nodes[node].parent = Some(0);
                self.nodes[0].children.push(node);
            }
        }
        for key in self.files.iter() {
            if is_markdown(key) && under(&self.base, key) && !listed.contains(key) {
                self.left_out.push(LeftOut {
                    path: key.clone(),
                    reason: "not_in_manifest",
                });
            }
        }
        self.manifest = Some(manifest);
    }
}

/// Whether a nav already lists `name` under `root`.
fn nav_mentions(items: &[NavItem], _root: &str, name: &str) -> bool {
    items.iter().any(|item| {
        item.path
            .as_deref()
            .is_some_and(|p| p.trim_start_matches("./").eq_ignore_ascii_case(name))
            || nav_mentions(&item.children, _root, name)
    })
}

fn reachable(nodes: &[Node]) -> HashSet<usize> {
    let mut seen = HashSet::new();
    let mut stack = vec![0];
    while let Some(n) = stack.pop() {
        if seen.insert(n) {
            stack.extend(nodes[n].children.iter().copied());
        }
    }
    seen
}

// From the tree to the plan.

/// What one page's file says about it before it is converted.
#[derive(Debug, Default)]
struct Peek {
    fields: Option<Fields>,
    front_matter_unreadable: bool,
    /// The text of an H1 that opens the file.
    first_heading: Option<String>,
    /// Link destinations in order, for Notion's child order.
    links: Vec<String>,
}

fn peek(markdown: &str, wikilinks: bool) -> Peek {
    use pulldown_cmark::{Event, HeadingLevel, MetadataBlockKind, Parser, Tag, TagEnd};
    let mut options = parse::options();
    if wikilinks {
        options |= pulldown_cmark::Options::ENABLE_WIKILINKS;
    }
    let mut peek = Peek::default();
    let mut metadata: Option<(MetadataBlockKind, String)> = None;
    let mut heading: Option<String> = None;
    let mut seen_block = false;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::MetadataBlock(kind)) if !seen_block => {
                metadata = Some((kind, String::new()));
            }
            Event::End(TagEnd::MetadataBlock(_)) => {
                if let Some((kind, raw)) = metadata.take() {
                    let block = FrontMatter {
                        kind: match kind {
                            MetadataBlockKind::YamlStyle => super::FrontMatterKind::Yaml,
                            MetadataBlockKind::PlusesStyle => super::FrontMatterKind::Pluses,
                        },
                        raw,
                    };
                    match front_matter::read(&block) {
                        Ok(fields) => peek.fields = Some(fields),
                        Err(_) => peek.front_matter_unreadable = true,
                    }
                }
            }
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) if !seen_block => {
                heading = Some(String::new());
                seen_block = true;
            }
            Event::End(TagEnd::Heading(HeadingLevel::H1)) => {
                if let Some(text) = heading.take() {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        peek.first_heading = Some(text);
                    }
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                peek.links.push(dest_url.to_string());
                seen_block = true;
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, raw)) = metadata.as_mut() {
                    raw.push_str(&t);
                } else if let Some(h) = heading.as_mut() {
                    h.push_str(&t);
                }
            }
            Event::Start(_) | Event::Rule | Event::Html(_) if metadata.is_none() => {
                seen_block = true;
            }
            _ => {}
        }
    }
    peek
}

/// The front matter keys the import reads.
const USED_KEYS: &[&str] = &[
    "title",
    "slug",
    "icon",
    "emoji",
    "draft",
    "status",
    "published",
    "weight",
    "nav_order",
    "order",
    "sidebar_position",
    "sidebar_label",
    "aliases",
    "uuid",
];

struct Planner<'a> {
    kind: ProfileKind,
    detected: Option<ProfileKind>,
    rules: Rules,
    options: &'a ImportOptions,
    workspace: &'a dyn Workspace,
    root: String,
    files: &'a [String],
    texts: &'a HashMap<String, String>,
    errors: &'a HashMap<String, String>,
    lossy: &'a HashSet<String>,
    manifest: Option<Manifest>,
    site_name: Option<String>,
    left_out: Vec<LeftOut>,
    read: &'a mut dyn FnMut(&str) -> Option<String>,
}

/// A node with what its file said, ready to order.
struct Draft {
    title: String,
    title_cut: bool,
    peek: Peek,
    order_key: Option<f64>,
    prefix: Option<u64>,
    notion_position: Option<usize>,
}

impl Planner<'_> {
    fn plan(mut self, mut nodes: Vec<Node>) -> Result<Built, Refused> {
        // What each file says.
        let mut drafts: Vec<Option<Draft>> = (0..nodes.len()).map(|_| None).collect();
        for (i, node) in nodes.iter().enumerate().skip(1) {
            let peek = node
                .file
                .as_ref()
                .and_then(|f| self.texts.get(f))
                .map(|t| peek(t, self.rules.wikilinks))
                .unwrap_or_default();
            let (title, title_cut) = self.title(node, &peek);
            let order_key = peek.fields.as_ref().and_then(|f| {
                ["weight", "nav_order", "order", "sidebar_position"]
                    .iter()
                    .find_map(|k| f.get(k).and_then(|v| v.as_f64()))
            });
            let name = node
                .file
                .as_deref()
                .map(|f| match &node.folder {
                    Some(folder) if is_index_name(f) => folder.rsplit('/').next().unwrap_or(folder),
                    _ => file_stem(f),
                })
                .or_else(|| {
                    node.folder
                        .as_deref()
                        .map(|f| f.rsplit('/').next().unwrap_or(f))
                })
                .unwrap_or("");
            let prefix = if self.rules.number_prefixes {
                number_prefix(self.clean_ids(name)).map(|(n, _)| n)
            } else {
                None
            };
            drafts[i] = Some(Draft {
                title,
                title_cut,
                peek,
                order_key,
                prefix,
                notion_position: None,
            });
        }

        // Notion: children in the order their parent links them.
        if self.kind == ProfileKind::Notion {
            for p in 1..nodes.len() {
                let Some(file) = nodes[p].file.clone() else {
                    continue;
                };
                let links = drafts[p]
                    .as_ref()
                    .map(|d| d.peek.links.clone())
                    .unwrap_or_default();
                let dir = parent(&file).to_string();
                let positions: HashMap<String, usize> = links
                    .iter()
                    .enumerate()
                    .filter_map(|(i, l)| {
                        let path = percent_decode(l.split(['#', '?']).next().unwrap_or(""));
                        join_normalised(&dir, &path).map(|k| (k.to_lowercase(), i))
                    })
                    .rev()
                    .collect();
                for &child in &nodes[p].children.clone() {
                    let keys = [nodes[child].file.clone(), nodes[child].folder.clone()];
                    let position = keys
                        .iter()
                        .flatten()
                        .find_map(|k| positions.get(&k.to_lowercase()).copied());
                    if let Some(d) = drafts[child].as_mut() {
                        d.notion_position = position;
                    }
                }
            }
        }

        // Order siblings.
        for n in 0..nodes.len() {
            let mut children = nodes[n].children.clone();
            children.sort_by(|a, b| self.compare(&nodes, &drafts, *a, *b));
            nodes[n].children = children;
        }

        // Collections, which may restructure the top level.
        let mode = match self.options.collections {
            Some(mode) => mode,
            None => match self.kind {
                ProfileKind::NosdeskV2 => CollectionMode::AsSource,
                ProfileKind::Outline => CollectionMode::PerTopLevelFolder,
                _ => CollectionMode::New,
            },
        };
        let (collections, mut top) = self.collections(mode, &mut nodes, &drafts);
        let excluded_files = self.leave_out_pages(&mut nodes, &drafts, &mut top);

        // Pages in tree order, parents first.
        let mut order: Vec<(usize, Option<usize>, u32, Option<usize>)> = Vec::new();
        let mut positions: HashMap<Option<usize>, u32> = HashMap::new();
        for (node, collection) in &top {
            let position = positions.entry(*collection).or_insert(0);
            walk(&nodes, *node, None, *position, *collection, &mut order);
            *position += 1;
        }
        if order.len() > MAX_PAGES {
            return Err(refused_too_many(order.len()));
        }
        let index_of: HashMap<usize, usize> = order
            .iter()
            .enumerate()
            .map(|(i, (node, ..))| (*node, i))
            .collect();

        // The link index.
        let mut index = LinkIndex::new(&self.root);
        for key in self.files {
            if !excluded_files.contains(key) {
                index.add_file(key);
            }
        }
        for (i, (node, ..)) in order.iter().enumerate() {
            let node_ref = &nodes[*node];
            let Some(draft) = drafts[*node].as_ref() else {
                continue;
            };
            let file = node_ref
                .file
                .as_deref()
                .filter(|f| self.texts.contains_key(*f));
            index.add_page(i, &draft.title, file, node_ref.folder.as_deref());
            if let Some(fields) = &draft.peek.fields {
                if let Some(aliases) = fields.get("aliases") {
                    for alias in aliases.strings() {
                        index.add_alias(&alias, i);
                    }
                }
                if let Some(slug) = fields.text("slug") {
                    index.add_slug(&slug, i);
                }
                if let Some(uuid) = fields.text("uuid") {
                    index.add_uuid(&uuid, i);
                }
            }
            if let Some(page) = &node_ref.manifest {
                if let Some(slug) = &page.slug {
                    index.add_slug(slug, i);
                }
                if let Some(uuid) = &page.uuid {
                    index.add_uuid(uuid, i);
                }
            }
        }
        let same_workspace = self
            .manifest
            .as_ref()
            .and_then(|m| m.workspace_uuid.as_deref())
            .is_some_and(|w| self.workspace.is_this_workspace(w));

        // Convert each page.
        let mut pages = Vec::with_capacity(order.len());
        let mut documents = Vec::with_capacity(order.len());
        let mut not_imported: BTreeMap<String, u32> = BTreeMap::new();
        let mut section_pages = 0;
        let mut totals = Notes::default();
        for (i, (node, parent_node, position, collection)) in order.iter().enumerate() {
            let node_ref = &nodes[*node];
            let draft = drafts[*node].as_ref().expect("every page node has a draft");
            let mut notes = Notes::default();
            if draft.title_cut {
                notes.add(NoteCode::TitleShortened);
            }
            if draft.peek.front_matter_unreadable {
                notes.add(NoteCode::FrontMatterIgnored);
            }
            let mut used = Vec::new();
            if let Some(fields) = &draft.peek.fields {
                for key in fields.keys() {
                    if USED_KEYS.contains(&key) {
                        used.push(key.to_string());
                    } else {
                        *not_imported.entry(key.to_string()).or_insert(0) += 1;
                    }
                }
            }
            let mut error = None;
            let (document, links) = match node_ref.file.as_ref() {
                Some(file) if self.texts.contains_key(file) => {
                    if self.lossy.contains(file) {
                        notes.add(NoteCode::NotUtf8);
                    }
                    self.convert(file, &draft.title, &index, same_workspace, &mut notes)
                }
                Some(file) => {
                    error = Some(
                        self.errors
                            .get(file)
                            .cloned()
                            .unwrap_or_else(|| "The file isn't in the zip.".to_string()),
                    );
                    (Document::default(), LinkReport::default())
                }
                None => {
                    let document = match &node_ref.csv {
                        Some(csv) => self.database(csv, &mut notes),
                        None => {
                            section_pages += 1;
                            Document::default()
                        }
                    };
                    let document =
                        normalise::normalise(document, &super::DefaultResolver, &mut notes);
                    (document, LinkReport::default())
                }
            };
            totals.merge(&notes);
            let kind = match (&node_ref.file, &node_ref.csv) {
                (Some(_), _) if parent_node.is_some_and(|p| nodes[p].csv.is_some()) => {
                    PageKind::DatabaseRow
                }
                (Some(_), _) => PageKind::Page,
                (None, Some(_)) => PageKind::Database,
                (None, None) => PageKind::Section,
            };
            let manifest = node_ref.manifest.as_ref();
            pages.push(PlannedPage {
                index: i,
                source: node_ref.file.clone(),
                folder: node_ref.folder.clone(),
                title: draft.title.clone(),
                parent: parent_node.and_then(|p| index_of.get(&p).copied()),
                order: *position,
                collection: *collection,
                kind,
                status: self.status(draft.peek.fields.as_ref(), manifest),
                icon: manifest
                    .and_then(|m| m.icon.clone())
                    .or_else(|| {
                        let fields = draft.peek.fields.as_ref()?;
                        fields.text("icon").or_else(|| fields.text("emoji"))
                    })
                    .filter(|i| i.chars().count() <= MAX_ICON_CHARS),
                slug: manifest
                    .and_then(|m| m.slug.clone())
                    .or_else(|| draft.peek.fields.as_ref()?.text("slug"))
                    .filter(|s| s.chars().count() <= MAX_TITLE_CHARS),
                uuid: manifest
                    .and_then(|m| m.uuid.clone())
                    .or_else(|| draft.peek.fields.as_ref()?.text("uuid")),
                front_matter_used: used,
                notes,
                links,
                error,
            });
            documents.push(document);
        }

        Ok(Built {
            plan: Plan {
                profile: self.kind,
                root: self.root.clone(),
                detected: self.detected,
                collections,
                pages,
                section_pages,
                front_matter_not_imported: not_imported,
                left_out: std::mem::take(&mut self.left_out),
                notes: totals,
            },
            documents,
        })
    }

    /// Take the pages the admin unticked out of the tree, with everything
    /// under them, before it is numbered. Returns the files they stood for
    /// (their Markdown and a database's CSV), which are left out: a link to
    /// one is kept as text, as for a file a pattern leaves out.
    fn leave_out_pages(
        &mut self,
        nodes: &mut [Node],
        drafts: &[Option<Draft>],
        top: &mut Vec<(usize, Option<usize>)>,
    ) -> HashSet<String> {
        let mut files = HashSet::new();
        if self.options.exclude.is_empty() {
            return files;
        }
        let mut gone = Vec::new();
        let mut trail = Vec::new();
        let options = self.options;
        let excluded = &options.exclude;
        top.retain(|(node, _)| {
            !drop_if_excluded(nodes, drafts, excluded, *node, &mut trail, &mut gone)
        });
        for node in gone {
            for key in [&nodes[node].file, &nodes[node].csv].into_iter().flatten() {
                if files.insert(key.clone()) {
                    self.left_out.push(LeftOut {
                        path: key.clone(),
                        reason: "excluded",
                    });
                }
            }
        }
        files
    }

    fn compare(&self, nodes: &[Node], drafts: &[Option<Draft>], a: usize, b: usize) -> Ordering {
        let (na, nb) = (&nodes[a], &nodes[b]);
        let (da, db) = (drafts[a].as_ref(), drafts[b].as_ref());
        let some_first = |x: Option<f64>, y: Option<f64>| match (x, y) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        let as_f = |x: Option<usize>| x.map(|v| v as f64);
        nb.first
            .cmp(&na.first)
            .then_with(|| some_first(as_f(na.nav_index), as_f(nb.nav_index)))
            .then_with(|| {
                some_first(
                    na.order.or(da.and_then(|d| d.order_key)),
                    nb.order.or(db.and_then(|d| d.order_key)),
                )
            })
            .then_with(|| {
                some_first(
                    as_f(da.and_then(|d| d.notion_position)),
                    as_f(db.and_then(|d| d.notion_position)),
                )
            })
            .then_with(|| {
                some_first(
                    da.and_then(|d| d.prefix).map(|p| p as f64),
                    db.and_then(|d| d.prefix).map(|p| p as f64),
                )
            })
            .then_with(|| {
                natural_cmp(
                    &da.map(|d| d.title.to_lowercase()).unwrap_or_default(),
                    &db.map(|d| d.title.to_lowercase()).unwrap_or_default(),
                )
            })
            .then_with(|| na.file.cmp(&nb.file))
            .then_with(|| a.cmp(&b))
    }

    /// The top-level pages, each with its collection, after applying the
    /// collection mode.
    fn collections(
        &self,
        mode: CollectionMode,
        nodes: &mut [Node],
        drafts: &[Option<Draft>],
    ) -> (Vec<PlannedCollection>, Vec<(usize, Option<usize>)>) {
        let roots = nodes[0].children.clone();
        let default_name = {
            self.options
                .collection_name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .or_else(|| self.site_name.clone())
                .or_else(|| {
                    // Notion's single top page names the export.
                    (self.kind == ProfileKind::Notion && roots.len() == 1)
                        .then(|| drafts[roots[0]].as_ref().map(|d| d.title.clone()))
                        .flatten()
                })
                .or_else(|| {
                    self.options
                        .archive_name
                        .as_deref()
                        .map(|n| file_stem(n).to_string())
                        .filter(|n| !n.trim().is_empty())
                })
                .unwrap_or_else(|| "Imported documents".to_string())
        };
        let new_collection = |name: String| PlannedCollection {
            name,
            slug: None,
            icon: None,
            color: None,
            description: None,
            source_uuid: None,
        };
        match mode {
            CollectionMode::Existing => (Vec::new(), roots.iter().map(|r| (*r, None)).collect()),
            CollectionMode::AsSource if self.manifest.is_some() => {
                let manifest = self.manifest.as_ref().expect("checked");
                let mut collections: Vec<(i32, usize, PlannedCollection)> = manifest
                    .collections
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        (
                            c.order.unwrap_or(i32::MAX),
                            i,
                            PlannedCollection {
                                name: c.name.clone(),
                                slug: c.slug.clone(),
                                icon: c.icon.clone(),
                                color: c.color.clone(),
                                description: c.description.clone(),
                                source_uuid: c.uuid.clone(),
                            },
                        )
                    })
                    .collect();
                collections.sort_by_key(|(order, i, _)| (*order, *i));
                let by_uuid: HashMap<String, usize> = collections
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (_, _, c))| {
                        c.source_uuid.as_ref().map(|u| (u.to_lowercase(), i))
                    })
                    .collect();
                let top = roots
                    .iter()
                    .map(|r| {
                        let collection = nodes[*r]
                            .manifest
                            .as_ref()
                            .and_then(|m| m.collection.as_ref())
                            .and_then(|u| by_uuid.get(&u.to_lowercase()).copied());
                        (*r, collection)
                    })
                    .collect();
                // Collections in their order, pages without one after.
                let mut top: Vec<(usize, Option<usize>)> = top;
                top.sort_by_key(|(_, c)| (c.is_none(), *c));
                (collections.into_iter().map(|(_, _, c)| c).collect(), top)
            }
            CollectionMode::PerTopLevelFolder => {
                let mut collections = Vec::new();
                let mut top = Vec::new();
                let mut loose: Option<usize> = None;
                for root in roots {
                    let node = &nodes[root];
                    if node.group {
                        let name = drafts[root]
                            .as_ref()
                            .map(|d| d.title.clone())
                            .unwrap_or_default();
                        collections.push(new_collection(name));
                        let collection = collections.len() - 1;
                        let has_content = node.file.is_some() || node.csv.is_some();
                        let children = node.children.clone();
                        if has_content {
                            // The folder's own page leads its collection.
                            nodes[root].children.clear();
                            top.push((root, Some(collection)));
                        }
                        for child in children {
                            nodes[child].parent = Some(0);
                            top.push((child, Some(collection)));
                        }
                    } else {
                        let collection = *loose.get_or_insert_with(|| {
                            collections.push(new_collection(default_name.clone()));
                            collections.len() - 1
                        });
                        top.push((root, Some(collection)));
                    }
                }
                (collections, top)
            }
            CollectionMode::New | CollectionMode::AsSource => (
                vec![new_collection(default_name)],
                roots.iter().map(|r| (*r, Some(0))).collect(),
            ),
        }
    }

    fn clean_ids<'n>(&self, name: &'n str) -> &'n str {
        if self.rules.notion_ids {
            strip_notion_id(name)
        } else {
            name
        }
    }

    /// The title, and whether it was cut to fit.
    fn title(&self, node: &Node, peek: &Peek) -> (String, bool) {
        let from_file = || -> Option<String> {
            let name = match (&node.file, &node.folder) {
                (Some(file), Some(folder)) if is_index_name(file) => {
                    folder.rsplit('/').next().unwrap_or(folder).to_string()
                }
                (Some(file), _) => file_stem(file).to_string(),
                (None, Some(folder)) => folder.rsplit('/').next().unwrap_or(folder).to_string(),
                (None, None) => return None,
            };
            let name = self.clean_ids(&name);
            let name = if self.rules.number_prefixes {
                number_prefix(name).map(|(_, rest)| rest).unwrap_or(name)
            } else {
                name
            };
            Some(humanise(name))
        };
        let title = node
            .manifest
            .as_ref()
            .map(|m| m.title.trim().to_string())
            .filter(|t| !t.is_empty())
            .or_else(|| peek.fields.as_ref().and_then(|f| f.text("title")))
            .or_else(|| {
                self.rules
                    .title_heading
                    .then(|| peek.first_heading.clone())
                    .flatten()
            })
            .or_else(|| node.label.clone().filter(|l| !l.trim().is_empty()))
            .or_else(|| peek.fields.as_ref().and_then(|f| f.text("sidebar_label")))
            .or_else(from_file)
            .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Untitled".to_string());
        if title.chars().count() > MAX_TITLE_CHARS {
            (title.chars().take(MAX_TITLE_CHARS).collect(), true)
        } else {
            (title, false)
        }
    }

    fn status(&self, fields: Option<&Fields>, manifest: Option<&ManifestPage>) -> PageStatus {
        match self.options.status {
            StatusMode::Drafts => PageStatus::Draft,
            StatusMode::Published => PageStatus::Published,
            StatusMode::AsFiles => {
                let status = manifest
                    .and_then(|m| m.status.clone())
                    .or_else(|| fields?.text("status"))
                    .map(|s| s.to_ascii_lowercase());
                match status.as_deref() {
                    Some("draft") => return PageStatus::Draft,
                    Some("archived") => return PageStatus::Archived,
                    Some("published") => return PageStatus::Published,
                    _ => {}
                }
                let draft = fields
                    .and_then(|f| f.get("draft"))
                    .and_then(|v| v.as_bool())
                    == Some(true);
                let unpublished = fields
                    .and_then(|f| f.get("published"))
                    .and_then(|v| v.as_bool())
                    == Some(false);
                if draft || unpublished {
                    PageStatus::Draft
                } else {
                    PageStatus::Published
                }
            }
        }
    }

    fn convert(
        &self,
        file: &str,
        title: &str,
        index: &LinkIndex,
        same_workspace: bool,
        notes: &mut Notes,
    ) -> (Document, LinkReport) {
        let text = &self.texts[file];
        let text = profiles::rewrite(text, &self.rules, notes);
        let resolver = PageResolver::new(index, parent(file), self.workspace, same_workspace);
        let parsed = parse::parse_with(
            &text,
            ParseOptions {
                wikilinks: self.rules.wikilinks,
            },
            &resolver,
        );
        notes.merge(&parsed.notes);
        let mut document = parsed.document;
        if self.rules.title_heading {
            remove_title_heading(&mut document, title);
        }
        let document = normalise::normalise(document, &resolver, notes);
        let (report, resolver_notes) = resolver.finish();
        notes.merge(&resolver_notes);
        (document, report)
    }

    /// A Notion database's page: its CSV as a table, the first rows only.
    fn database(&mut self, key: &str, notes: &mut Notes) -> Document {
        let Some(text) = (self.read)(key) else {
            return Document::default();
        };
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(text.trim_start_matches('\u{feff}').as_bytes());
        let mut rows: Vec<Vec<TableCell>> = Vec::new();
        let mut cut = false;
        for (r, record) in reader.records().enumerate() {
            let Ok(record) = record else {
                break;
            };
            if r > DATABASE_ROWS {
                cut = true;
                break;
            }
            rows.push(
                record
                    .iter()
                    .map(|cell| TableCell::new(r == 0, vec![Inline::text(cell.trim())]))
                    .collect(),
            );
        }
        if cut {
            notes.add(NoteCode::DatabaseRowsCut);
        }
        if rows.is_empty() {
            return Document::default();
        }
        Document {
            blocks: vec![Block::Table(Table { rows })],
        }
    }
}

/// Whether `node` is excluded; if so, it and everything under it go in
/// `gone`, and otherwise its excluded children leave it.
fn drop_if_excluded(
    nodes: &mut [Node],
    drafts: &[Option<Draft>],
    excluded: &ExcludedPages,
    node: usize,
    trail: &mut Vec<String>,
    gone: &mut Vec<usize>,
) -> bool {
    trail.push(
        drafts[node]
            .as_ref()
            .map(|d| d.title.clone())
            .unwrap_or_default(),
    );
    let hit = excluded.matches(
        nodes[node].file.as_deref(),
        nodes[node].folder.as_deref(),
        trail,
    );
    if hit {
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            gone.push(n);
            stack.extend(nodes[n].children.iter().copied());
        }
    } else {
        let children = nodes[node].children.clone();
        let kept: Vec<usize> = children
            .into_iter()
            .filter(|c| !drop_if_excluded(nodes, drafts, excluded, *c, trail, gone))
            .collect();
        nodes[node].children = kept;
    }
    trail.pop();
    hit
}

fn walk(
    nodes: &[Node],
    node: usize,
    parent: Option<usize>,
    position: u32,
    collection: Option<usize>,
    out: &mut Vec<(usize, Option<usize>, u32, Option<usize>)>,
) {
    out.push((node, parent, position, collection));
    for (i, child) in nodes[node].children.iter().enumerate() {
        walk(nodes, *child, Some(node), i as u32, None, out);
    }
}

fn is_index_name(file: &str) -> bool {
    matches!(
        file_stem(file).to_ascii_lowercase().as_str(),
        "index" | "readme" | "_index"
    )
}

/// Remove an H1 that opens the document and repeats the title.
fn remove_title_heading(document: &mut Document, title: &str) {
    if let Some(Block::Heading { level: 1, content }) = document.blocks.first() {
        let text = super::ir::plain_text(content);
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text == title {
            document.blocks.remove(0);
        }
    }
}

/// A number prefix (`01-`, `1. `, `010_`) and the name after it.
pub fn number_prefix(name: &str) -> Option<(u64, &str)> {
    let digits = name.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let rest = &name[digits..];
    let after = rest
        .strip_prefix(['-', '_', '.'])
        .map(str::trim_start)
        .or_else(|| rest.strip_prefix(' ').map(str::trim_start))?;
    if after.is_empty() {
        return None;
    }
    Some((name[..digits].parse().ok()?, after))
}

/// A file name as a title: `kebab-case` and `snake_case` as words, and the
/// first letter capitalised, so `vpn-setup` is "Vpn setup". Names with
/// spaces keep their separators.
pub fn humanise(name: &str) -> String {
    let name = name.trim();
    let words = if name.contains(' ') || !name.contains(['-', '_']) {
        name.to_string()
    } else {
        name.split(['-', '_'])
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Natural order: digit runs compare as numbers.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a = a.chars().peekable();
    let mut b = b.chars().peekable();
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = a.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    a.next();
                }
                let mut nb = String::new();
                while let Some(c) = b.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    b.next();
                }
                let ta = na.trim_start_matches('0');
                let tb = nb.trim_start_matches('0');
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                a.next();
                b.next();
            }
        }
    }
}

/// A glob over archive paths: `**` crosses folders (`**/` is any number of
/// whole folders), `*` and `?` don't. A pattern with no `/` matches any one
/// name in the path. Matching is linear in the pattern times the path.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern.len() > 512 {
        return false;
    }
    let tokens = glob_tokens(pattern.trim_start_matches('/'));
    if !pattern.contains('/') {
        return path
            .split('/')
            .any(|segment| glob(&tokens, segment.as_bytes()));
    }
    glob(&tokens, path.as_bytes())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlobToken {
    Byte(u8),
    /// `?`: one byte other than `/`.
    One,
    /// `*`: any run without `/`.
    Star,
    /// `**`: any run.
    Any,
    /// `**/`: nothing, or any run ending in `/`.
    Folders,
}

fn glob_tokens(pattern: &str) -> Vec<GlobToken> {
    let bytes = pattern.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'*' if bytes.get(i + 1) == Some(&b'*') => {
                if bytes.get(i + 2) == Some(&b'/') {
                    tokens.push(GlobToken::Folders);
                    i += 3;
                } else {
                    tokens.push(GlobToken::Any);
                    i += 2;
                }
            }
            b'*' => {
                tokens.push(GlobToken::Star);
                i += 1;
            }
            b'?' => {
                tokens.push(GlobToken::One);
                i += 1;
            }
            b => {
                tokens.push(GlobToken::Byte(b));
                i += 1;
            }
        }
    }
    tokens
}

/// Whether `tokens` match all of `s`, by dynamic programming from the end:
/// `next[j]` says the tokens after this one match `s[j..]`.
fn glob(tokens: &[GlobToken], s: &[u8]) -> bool {
    let n = s.len();
    let mut next = vec![false; n + 1];
    next[n] = true;
    for token in tokens.iter().rev() {
        let mut row = vec![false; n + 1];
        // For `Folders`: some `/` at or after j is followed by a match.
        let mut slash_then_match = false;
        for j in (0..=n).rev() {
            row[j] = match token {
                GlobToken::Byte(b) => j < n && s[j] == *b && next[j + 1],
                GlobToken::One => j < n && s[j] != b'/' && next[j + 1],
                GlobToken::Star => next[j] || (j < n && s[j] != b'/' && row[j + 1]),
                GlobToken::Any => next[j] || (j < n && row[j + 1]),
                GlobToken::Folders => {
                    if j < n && s[j] == b'/' && next[j + 1] {
                        slash_then_match = true;
                    }
                    next[j] || (j < n && slash_then_match)
                }
            };
        }
        next = row;
    }
    next[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_prefixes_order_and_strip() {
        assert_eq!(number_prefix("01-intro"), Some((1, "intro")));
        assert_eq!(
            number_prefix("1. Getting started"),
            Some((1, "Getting started"))
        );
        assert_eq!(number_prefix("010_setup"), Some((10, "setup")));
        assert_eq!(number_prefix("2024"), None);
        assert_eq!(number_prefix("3d-printing"), None);
    }

    #[test]
    fn names_are_humanised() {
        assert_eq!(humanise("vpn-setup"), "Vpn setup");
        assert_eq!(humanise("snake_case_name"), "Snake case name");
        assert_eq!(humanise("VPN Setup"), "VPN Setup");
        assert_eq!(humanise("README"), "README");
        assert_eq!(humanise("policies"), "Policies");
    }

    #[test]
    fn natural_sort_reads_numbers() {
        let mut names = vec!["page 10", "page 2", "Page 1", "alpha"];
        names.sort_by(|a, b| natural_cmp(&a.to_lowercase(), &b.to_lowercase()));
        assert_eq!(names, vec!["alpha", "Page 1", "page 2", "page 10"]);
    }

    #[test]
    fn globs_match_paths() {
        assert!(glob_match("**/archive/**", "docs/archive/old.md"));
        assert!(glob_match("**/archive/**", "archive/old.md"));
        assert!(!glob_match("**/archive/**", "docs/archived.md"));
        assert!(glob_match("drafts", "a/drafts/b.md"));
        assert!(glob_match("*.tmp.md", "a/x.tmp.md"));
        assert!(glob_match("docs/*.md", "docs/a.md"));
        assert!(!glob_match("docs/*.md", "docs/sub/a.md"));
        assert!(!glob_match("**/archive/**", "docs/xarchive/old.md"));
        assert!(glob_match("docs/**", "docs/a/b/c.md"));
        assert!(glob_match("?.md", "a/b.md"));
        let hostile = "*a".repeat(40) + "b";
        assert!(!glob_match(&hostile, &"a".repeat(2000)));
    }
}
