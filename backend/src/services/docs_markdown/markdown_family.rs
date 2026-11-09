//! The Markdown family: every profile that exports a tree of Markdown
//! files, read through the [`DocsSource`] seam.
//!
//! A profile lays out the tree: a manifest (Nosdesk), a SUMMARY or nav
//! (GitBook, MkDocs), `_category_` files (Docusaurus) or the folders
//! themselves. Each file is then peeked for its front matter and an opening
//! H1, which name, order and describe the page.
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
use std::collections::{HashMap, HashSet};

use super::front_matter::{self, Fields};
use super::html;
use super::ir::{Block, Document, Inline, Table, TableCell};
use super::links::{file_stem, join_normalised, parent, percent_decode, strip_notion_id};
use super::parse::{self, ParseOptions};
use super::profiles::{self, is_markdown, Manifest, ManifestPage, NavItem, ProfileKind, Rules};
use super::source::{Body, DocsSource, LinkNames, SourceInput, SourcePage, SourceRefs, SourceTree};
use super::tree::{
    fit_title, humanise, natural_cmp, number_prefix, refused_too_many, CollectionMode, LeftOut,
    PageKind, PageStatus, PlannedCollection, DATABASE_ROWS, MAX_CONFIG_BYTES, MAX_MARKDOWN_BYTES,
    MAX_PAGES,
};
use super::{FrontMatter, NoteCode, Notes, Resolver};
use crate::services::imports::refusal::Refused;
use crate::services::uploaded_archive::SafeArchive;

fn refused_no_markdown() -> Refused {
    Refused::new(
        "docs_import_no_markdown",
        "There are no Markdown files in this zip.",
    )
}

/// The archive's Markdown files, read once: detection looks at them before
/// the family lays them out.
#[derive(Debug, Default)]
pub struct MarkdownFiles {
    /// Every Markdown key outside a tool's folder.
    keys: Vec<String>,
    /// The text of each one read.
    pub texts: HashMap<String, String>,
    /// Why each one not read couldn't be.
    errors: HashMap<String, String>,
    /// Those read with characters that weren't UTF-8.
    lossy: HashSet<String>,
}

impl MarkdownFiles {
    /// Read every Markdown file among `files`, refusing an archive with
    /// more than an import may hold.
    pub fn read(archive: &mut SafeArchive, files: &[String]) -> Result<Self, Refused> {
        let keys: Vec<String> = files
            .iter()
            .filter(|k| is_markdown(k) && !in_tool_folder(k))
            .cloned()
            .collect();
        if keys.len() > MAX_PAGES {
            return Err(refused_too_many(keys.len()));
        }
        let mut read = MarkdownFiles::default();
        for key in &keys {
            match archive.read_text(key, MAX_MARKDOWN_BYTES) {
                Ok(text) => {
                    if text.lossy {
                        read.lossy.insert(key.clone());
                    }
                    read.texts.insert(key.clone(), text.text);
                }
                Err(e) => {
                    read.errors.insert(key.clone(), e.to_string());
                }
            }
        }
        read.keys = keys;
        Ok(read)
    }
}

/// A Markdown export under one profile.
pub struct MarkdownFamily {
    kind: ProfileKind,
    rules: Rules,
    /// The folder detection found the source under.
    base: String,
    markdown: MarkdownFiles,
}

impl MarkdownFamily {
    pub fn new(kind: ProfileKind, base: String, markdown: MarkdownFiles) -> Self {
        MarkdownFamily {
            kind,
            rules: kind.rules(),
            base,
            markdown,
        }
    }
}

impl DocsSource for MarkdownFamily {
    fn kind(&self) -> ProfileKind {
        self.kind
    }

    fn read(&mut self, input: &mut SourceInput<'_>) -> Result<SourceTree, Refused> {
        let SourceInput::Archive { archive, files } = input;
        let files: &[String] = files;
        if self.markdown.keys.is_empty() {
            return Err(refused_no_markdown());
        }
        let mut read_small = |key: &str| {
            archive
                .read_text(key, MAX_CONFIG_BYTES)
                .ok()
                .map(|t| t.text)
        };

        let mut layout = Layout {
            kind: self.kind,
            rules: self.rules,
            base: self.base.clone(),
            files,
            texts: &self.markdown.texts,
            read: &mut read_small,
            nodes: vec![Node::root()],
            left_out: Vec::new(),
            manifest: None,
            site_name: None,
        };
        layout.lay_out();
        let Layout {
            mut nodes,
            manifest,
            site_name,
            base,
            left_out,
            ..
        } = layout;

        // What each file says.
        let mut drafts: Vec<Option<Draft>> = (0..nodes.len()).map(|_| None).collect();
        for (i, node) in nodes.iter().enumerate().skip(1) {
            let peek = node
                .file
                .as_ref()
                .and_then(|f| self.markdown.texts.get(f))
                .map(|t| peek(t, self.rules.wikilinks))
                .unwrap_or_default();
            let title = self.title(node, &peek);
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
                // Siblings sort by the title the planner will give them.
                sort_title: fit_title(&title).0,
                title,
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
            children.sort_by(|a, b| compare(&nodes, &drafts, *a, *b));
            nodes[n].children = children;
        }

        // The manifest's collections, in their order.
        let collections = manifest.as_ref().map(|m| {
            let mut collections: Vec<(i32, usize, PlannedCollection)> = m
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
            collections
                .into_iter()
                .map(|(_, _, c)| c)
                .collect::<Vec<_>>()
        });
        let collection_of_uuid: HashMap<String, usize> = collections
            .iter()
            .flatten()
            .enumerate()
            .filter_map(|(i, c)| c.source_uuid.as_ref().map(|u| (u.to_lowercase(), i)))
            .collect();

        // Notion's single top page names the export.
        let roots = &nodes[0].children;
        let site_name = site_name.or_else(|| {
            (self.kind == ProfileKind::Notion && roots.len() == 1)
                .then(|| drafts[roots[0]].as_ref().map(|d| d.sort_title.clone()))
                .flatten()
        });

        // Each node but the root as a page, node `n` at `n - 1`. A file
        // more than one node names (a manifest listing it twice) is copied;
        // otherwise its text moves.
        let mut uses: HashMap<String, usize> = HashMap::new();
        for file in nodes.iter().filter_map(|n| n.file.as_ref()) {
            *uses.entry(file.clone()).or_insert(0) += 1;
        }
        let mut pages = Vec::with_capacity(nodes.len().saturating_sub(1));
        for (i, node) in nodes.iter().enumerate().skip(1) {
            let draft = drafts[i].take().expect("every page node has a draft");
            let fields = draft.peek.fields.as_ref();
            let manifest_page = node.manifest.as_ref();
            let mut notes = Notes::default();
            if draft.peek.front_matter_unreadable {
                notes.add(NoteCode::FrontMatterIgnored);
            }
            let mut used = Vec::new();
            let mut not_imported = Vec::new();
            for key in fields.into_iter().flat_map(Fields::keys) {
                if USED_KEYS.contains(&key) {
                    used.push(key.to_string());
                } else {
                    not_imported.push(key.to_string());
                }
            }
            let mut error = None;
            let body =
                match (&node.file, &node.csv) {
                    (Some(file), _) => {
                        let text = match uses.get_mut(file) {
                            Some(n) if *n > 1 => {
                                *n -= 1;
                                self.markdown.texts.get(file).cloned()
                            }
                            _ => self.markdown.texts.remove(file),
                        };
                        match text {
                            Some(text) => {
                                if self.markdown.lossy.contains(file) {
                                    notes.add(NoteCode::NotUtf8);
                                }
                                Body::Markdown(text)
                            }
                            None => {
                                error =
                                    Some(self.markdown.errors.get(file).cloned().unwrap_or_else(
                                        || "The file isn't in the zip.".to_string(),
                                    ));
                                Body::Empty
                            }
                        }
                    }
                    (None, Some(csv)) => Body::Csv(read_small(csv)),
                    (None, None) => Body::Empty,
                };
            let parent_is_database = node.parent.is_some_and(|p| nodes[p].csv.is_some());
            let kind = match (&node.file, &node.csv) {
                (Some(_), _) if parent_is_database => PageKind::DatabaseRow,
                (Some(_), _) => PageKind::Page,
                (None, Some(_)) => PageKind::Database,
                (None, None) => PageKind::Section,
            };
            let mut names = LinkNames::default();
            if let Some(fields) = fields {
                if let Some(aliases) = fields.get("aliases") {
                    names.aliases = aliases.strings();
                }
                names.slugs.extend(fields.text("slug"));
                names.uuids.extend(fields.text("uuid"));
            }
            if let Some(page) = manifest_page {
                names.slugs.extend(page.slug.clone());
                names.uuids.extend(page.uuid.clone());
            }
            pages.push(SourcePage {
                key: node
                    .file
                    .clone()
                    .or_else(|| node.folder.clone())
                    .unwrap_or_default(),
                file: node.file.clone(),
                folder: node.folder.clone(),
                entries: [&node.file, &node.csv]
                    .into_iter()
                    .flatten()
                    .cloned()
                    .collect(),
                title: draft.title,
                children: node.children.iter().map(|c| c - 1).collect(),
                group: node.group,
                collection: manifest_page
                    .and_then(|m| m.collection.as_ref())
                    .and_then(|u| collection_of_uuid.get(&u.to_lowercase()).copied()),
                kind,
                body,
                error,
                status: Some(file_status(fields, manifest_page)),
                icon: manifest_page.and_then(|m| m.icon.clone()).or_else(|| {
                    let fields = fields?;
                    fields.text("icon").or_else(|| fields.text("emoji"))
                }),
                slug: manifest_page
                    .and_then(|m| m.slug.clone())
                    .or_else(|| fields?.text("slug")),
                uuid: manifest_page
                    .and_then(|m| m.uuid.clone())
                    .or_else(|| fields?.text("uuid")),
                names,
                metadata_used: used,
                metadata_not_imported: not_imported,
                notes,
            });
        }

        Ok(SourceTree {
            root: base,
            site_name,
            default_collections: match self.kind {
                ProfileKind::NosdeskV2 => CollectionMode::AsSource,
                ProfileKind::Outline => CollectionMode::PerTopLevelFolder,
                _ => CollectionMode::New,
            },
            collections,
            top: nodes[0].children.iter().map(|c| c - 1).collect(),
            pages,
            files: files.to_vec(),
            left_out,
            workspace_uuid: manifest.and_then(|m| m.workspace_uuid),
            refs: SourceRefs::default(),
        })
    }

    fn convert(&self, page: &SourcePage, resolver: &dyn Resolver, notes: &mut Notes) -> Document {
        match &page.body {
            Body::Markdown(text) => {
                let text = profiles::rewrite(text, &self.rules, notes);
                let parsed = parse::parse_with(
                    &text,
                    ParseOptions {
                        wikilinks: self.rules.wikilinks,
                    },
                    resolver,
                );
                notes.merge(&parsed.notes);
                let mut document = parsed.document;
                if self.rules.title_heading {
                    remove_title_heading(&mut document, &page.title);
                }
                document
            }
            // A Markdown family reads no HTML pages; one would read as plain
            // HTML.
            Body::Html(html) => {
                html::read_document(html, &[], &html::DefaultDialect, resolver, notes)
            }
            Body::Csv(Some(text)) => database(text, notes),
            Body::Csv(None) | Body::Empty => Document::default(),
        }
    }
}

impl MarkdownFamily {
    fn clean_ids<'n>(&self, name: &'n str) -> &'n str {
        if self.rules.notion_ids {
            strip_notion_id(name)
        } else {
            name
        }
    }

    /// The title in full; empty when nothing names the page.
    fn title(&self, node: &Node, peek: &Peek) -> String {
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
        node.manifest
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
            .unwrap_or_default()
    }
}

/// The status a page's front matter or manifest entry gives it.
fn file_status(fields: Option<&Fields>, manifest: Option<&ManifestPage>) -> PageStatus {
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

/// A Notion database's page: its CSV as a table, the first rows only.
fn database(text: &str, notes: &mut Notes) -> Document {
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

/// A profile laying out the archive's files as nodes under a root.
struct Layout<'a> {
    kind: ProfileKind,
    rules: Rules,
    base: String,
    files: &'a [String],
    texts: &'a HashMap<String, String>,
    read: &'a mut dyn FnMut(&str) -> Option<String>,
    nodes: Vec<Node>,
    left_out: Vec<LeftOut>,
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

impl Layout<'_> {
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
            // A BookStack export never reaches the Markdown family; read as
            // a plain folder if it did.
            ProfileKind::Notion
            | ProfileKind::Obsidian
            | ProfileKind::Outline
            | ProfileKind::Plain
            | ProfileKind::Bookstack => {
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

// What each file says, and the order it gives.

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

/// A node with what its file said, ready to order.
struct Draft {
    title: String,
    sort_title: String,
    peek: Peek,
    order_key: Option<f64>,
    prefix: Option<u64>,
    notion_position: Option<usize>,
}

fn compare(nodes: &[Node], drafts: &[Option<Draft>], a: usize, b: usize) -> Ordering {
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
                &da.map(|d| d.sort_title.to_lowercase()).unwrap_or_default(),
                &db.map(|d| d.sort_title.to_lowercase()).unwrap_or_default(),
            )
        })
        .then_with(|| na.file.cmp(&nb.file))
        .then_with(|| a.cmp(&b))
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
