//! An uploaded archive as a plan of pages, and each page as a document.
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
//! It works in two halves either side of [`super::source`]: a source reads
//! the archive into a [`SourceTree`] (today every profile is a
//! [`MarkdownFamily`]), and the planner here turns that tree into the plan
//! under the admin's options, converting each page through the source.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::bookstack::{self, BookstackSource, Exports};
use super::ir::Document;
use super::links::RefTarget;
use super::links::{file_stem, parent, LinkIndex, LinkReport, PageResolver, Workspace};
use super::markdown_family::{MarkdownFamily, MarkdownFiles};
use super::profiles::{self, Detection, Listing, ProfileKind};
use super::source::{DocsSource, SourceInput, SourcePage, SourceRef, SourceTree};
use super::{normalise, NoteCode, Notes};
use crate::services::imports::refusal::Refused;
use crate::services::uploaded_archive::{SafeArchive, SkipReason};

/// Pages one import may hold.
pub const MAX_PAGES: usize = 2_000;
/// Largest Markdown file read.
pub const MAX_MARKDOWN_BYTES: u64 = 2 * 1024 * 1024;
/// Largest config file or database CSV read.
pub(super) const MAX_CONFIG_BYTES: u64 = 10 * 1024 * 1024;
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
/// page goes when its file or its folder is listed; for a page with
/// neither, when its source key is (a BookStack page), else its titles from
/// the top down (a nav heading).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExcludedPages {
    pub files: BTreeSet<String>,
    pub folders: BTreeSet<String>,
    pub trails: BTreeSet<Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub keys: BTreeSet<String>,
}

impl ExcludedPages {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
            && self.folders.is_empty()
            && self.trails.is_empty()
            && self.keys.is_empty()
    }

    /// Leave out the page with this file, folder, source key and trail of
    /// titles. A page with no file or folder goes by its key when it has
    /// one, so same-titled siblings stay apart.
    pub fn add(&mut self, file: Option<&str>, folder: Option<&str>, key: &str, trail: &[String]) {
        if let Some(file) = file {
            self.files.insert(file.to_string());
        }
        if let Some(folder) = folder {
            self.folders.insert(folder.to_string());
        }
        if file.is_none() && folder.is_none() {
            if key.is_empty() {
                self.trails.insert(trail.to_vec());
            } else {
                self.keys.insert(key.to_string());
            }
        }
    }

    pub fn matches(
        &self,
        file: Option<&str>,
        folder: Option<&str>,
        key: &str,
        trail: &[String],
    ) -> bool {
        match (file, folder) {
            (None, None) => {
                (!key.is_empty() && self.keys.contains(key)) || self.trails.contains(trail)
            }
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
    /// What a later import finds the page by: [`SourcePage::key`].
    pub key: String,
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

pub(super) fn refused_too_many(count: usize) -> Refused {
    Refused::new(
        "docs_import_too_many_pages",
        format!(
            "The import has {count} pages. The limit is {MAX_PAGES}. Export one part at a time."
        ),
    )
    .arg("count", count)
    .arg("max", MAX_PAGES)
}

/// A title as a page holds it: whitespace runs as one space, "Untitled"
/// when empty, at most [`MAX_TITLE_CHARS`]. Also says whether it was cut.
pub fn fit_title(title: &str) -> (String, bool) {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = if title.is_empty() {
        "Untitled".to_string()
    } else {
        title
    };
    if title.chars().count() > MAX_TITLE_CHARS {
        (title.chars().take(MAX_TITLE_CHARS).collect(), true)
    } else {
        (title, false)
    }
}

/// Read `archive` into a plan and documents. Reads a BookStack export's
/// `data.json`, or every Markdown file, once; nothing is written anywhere.
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
    let mut detected_bookstack = false;
    let ignored: Vec<String> = archive
        .skipped()
        .iter()
        .filter(|s| s.reason == SkipReason::Ignored)
        .map(|s| s.key.clone())
        .collect();

    // A BookStack export is found by its data.json, read once under its own
    // cap, before any Markdown is read. A forced profile reads what
    // detection would refuse.
    let bookstack = Exports::find(archive, &files);
    match (options.profile, bookstack) {
        (None | Some(ProfileKind::Bookstack), Ok(Some(exports))) => {
            let mut source = BookstackSource::new(exports);
            let tree = source.read(&mut SourceInput::Archive {
                archive,
                files: &files,
            })?;
            return Planner {
                source: &source,
                detected: None,
                options,
                workspace,
                left_out,
            }
            .plan(tree);
        }
        (Some(ProfileKind::Bookstack), Ok(None)) => return Err(bookstack::refused_unreadable()),
        (None | Some(ProfileKind::Bookstack), Err(refused)) => return Err(refused),
        (Some(_), Ok(Some(_))) => detected_bookstack = true,
        (Some(_), Err(_) | Ok(None)) | (None, Ok(None)) => {}
    }

    let markdown = MarkdownFiles::read(archive, &files)?;
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
        markdown: &markdown.texts,
    });
    let (kind, detected, base) = match options.profile {
        // A forced profile reads what detection would refuse, and says what
        // detection thought, for "Change".
        Some(kind) => {
            let base = detection
                .as_ref()
                .map(|d| d.base.clone())
                .unwrap_or_else(|_| profiles::wrapper_folder(&files));
            let detected = if detected_bookstack {
                Some(ProfileKind::Bookstack)
            } else {
                detection.ok().map(|d| d.kind).filter(|d| *d != kind)
            };
            (kind, detected, base)
        }
        None => {
            let Detection { kind, base } = detection?;
            (kind, None, base)
        }
    };

    let mut source = MarkdownFamily::new(kind, base, markdown);
    let tree = source.read(&mut SourceInput::Archive {
        archive,
        files: &files,
    })?;
    Planner {
        source: &source,
        detected,
        options,
        workspace,
        left_out,
    }
    .plan(tree)
}

// From the source's tree to the plan.

/// What every source shares: the admin's options applied to a
/// [`SourceTree`], and each page converted through its source.
struct Planner<'a> {
    source: &'a dyn DocsSource,
    detected: Option<ProfileKind>,
    options: &'a ImportOptions,
    workspace: &'a dyn Workspace,
    /// Files left out so far: those the leave-out patterns matched.
    left_out: Vec<LeftOut>,
}

impl Planner<'_> {
    fn plan(mut self, tree: SourceTree) -> Result<Built, Refused> {
        let SourceTree {
            root,
            site_name,
            default_collections,
            collections: source_collections,
            mut pages,
            top: roots,
            files,
            left_out,
            workspace_uuid,
            refs,
        } = tree;
        self.left_out.extend(left_out);

        // Titles as the pages will hold them.
        let mut shortened = vec![false; pages.len()];
        for (page, cut) in pages.iter_mut().zip(shortened.iter_mut()) {
            let (title, was_cut) = fit_title(&page.title);
            page.title = title;
            *cut = was_cut;
        }

        // Collections, which may restructure the top level.
        let mode = self.options.collections.unwrap_or(default_collections);
        let (mut collections, mut top) =
            self.collections(mode, &mut pages, roots, site_name, source_collections);
        let excluded_files = self.leave_out_pages(&mut pages, &mut top);
        if !self.options.exclude.is_empty() {
            // Unticked pages don't bring their collections along: only the
            // collections a kept page goes in are created.
            collections = keep_used_collections(collections, &mut top);
        }

        // Pages in tree order, parents first.
        let mut order: Vec<(usize, Option<usize>, u32, Option<usize>)> = Vec::new();
        let mut positions: HashMap<Option<usize>, u32> = HashMap::new();
        for (page, collection) in &top {
            let position = positions.entry(*collection).or_insert(0);
            walk(&pages, *page, None, *position, *collection, &mut order);
            *position += 1;
        }
        if order.len() > MAX_PAGES {
            return Err(refused_too_many(order.len()));
        }
        let index_of: HashMap<usize, usize> = order
            .iter()
            .enumerate()
            .map(|(i, (page, ..))| (*page, i))
            .collect();

        // The link index.
        let mut index = LinkIndex::new(&root);
        for key in &files {
            if !excluded_files.contains(key) {
                index.add_file(key);
            }
        }
        for (i, (page, ..)) in order.iter().enumerate() {
            let page = &pages[*page];
            let file = page.file.as_deref().filter(|_| page.error.is_none());
            index.add_page(i, &page.title, file, page.folder.as_deref());
            for alias in &page.names.aliases {
                index.add_alias(alias, i);
            }
            for slug in &page.names.slugs {
                index.add_slug(slug, i);
            }
            for uuid in &page.names.uuids {
                index.add_uuid(uuid, i);
            }
        }
        // The source's own destinations, by where they ended up in the plan.
        // A link to a source collection goes to the collection its first
        // top-level page went in.
        for scheme in &refs.schemes {
            index.add_ref_scheme(scheme);
        }
        let mut collection_pages: HashMap<usize, usize> = HashMap::new();
        for (i, (page, parent_page, ..)) in order.iter().enumerate() {
            if let (None, Some(c)) = (parent_page, pages[*page].collection) {
                collection_pages.entry(c).or_insert(i);
            }
        }
        let readable: HashSet<&str> = files
            .iter()
            .filter(|key| !excluded_files.contains(*key))
            .map(String::as_str)
            .collect();
        for (href, target) in refs.targets {
            let target = match target {
                SourceRef::Page(page) => index_of.get(&page).copied().map(RefTarget::Page),
                SourceRef::Collection(c) => {
                    collection_pages.get(&c).copied().map(RefTarget::Collection)
                }
                SourceRef::Image(key) => readable
                    .contains(key.as_str())
                    .then_some(RefTarget::Image(key)),
                SourceRef::Attachment(name) => Some(RefTarget::Attachment(name)),
                SourceRef::Web(url) => Some(RefTarget::Web(url)),
            };
            if let Some(target) = target {
                index.add_ref(&href, target);
            }
        }
        let same_workspace = workspace_uuid
            .as_deref()
            .is_some_and(|w| self.workspace.is_this_workspace(w));

        // Convert each page.
        let mut planned = Vec::with_capacity(order.len());
        let mut documents = Vec::with_capacity(order.len());
        let mut not_imported: BTreeMap<String, u32> = BTreeMap::new();
        let mut section_pages = 0;
        let mut totals = Notes::default();
        for (i, (p, parent_page, position, collection)) in order.iter().enumerate() {
            let page = &pages[*p];
            let mut notes = page.notes.clone();
            if shortened[*p] {
                notes.add(NoteCode::TitleShortened);
            }
            for key in &page.metadata_not_imported {
                *not_imported.entry(key.clone()).or_insert(0) += 1;
            }
            let (document, links) =
                self.convert(page, &index, same_workspace, &mut notes, &mut section_pages);
            totals.merge(&notes);
            // A row whose database isn't its parent any more is a page.
            let kind = match page.kind {
                PageKind::DatabaseRow
                    if !parent_page.is_some_and(|q| pages[q].kind == PageKind::Database) =>
                {
                    PageKind::Page
                }
                kind => kind,
            };
            planned.push(PlannedPage {
                index: i,
                key: page.key.clone(),
                source: page.file.clone(),
                folder: page.folder.clone(),
                title: page.title.clone(),
                parent: parent_page.and_then(|q| index_of.get(&q).copied()),
                order: *position,
                collection: *collection,
                kind,
                status: match self.options.status {
                    StatusMode::Drafts => PageStatus::Draft,
                    StatusMode::Published => PageStatus::Published,
                    StatusMode::AsFiles => page.status.unwrap_or(PageStatus::Published),
                },
                icon: page
                    .icon
                    .clone()
                    .filter(|i| i.chars().count() <= MAX_ICON_CHARS),
                slug: page
                    .slug
                    .clone()
                    .filter(|s| s.chars().count() <= MAX_TITLE_CHARS),
                uuid: page.uuid.clone(),
                front_matter_used: page.metadata_used.clone(),
                notes,
                links,
                error: page.error.clone(),
            });
            documents.push(document);
        }

        Ok(Built {
            plan: Plan {
                profile: self.source.kind(),
                root,
                detected: self.detected,
                collections,
                pages: planned,
                section_pages,
                front_matter_not_imported: not_imported,
                left_out: std::mem::take(&mut self.left_out),
                notes: totals,
            },
            documents,
        })
    }

    /// One page's document and link report. A page whose content couldn't
    /// be read is imported empty; one without links of its own (a section,
    /// a database) resolves nothing.
    fn convert(
        &self,
        page: &SourcePage,
        index: &LinkIndex,
        same_workspace: bool,
        notes: &mut Notes,
        section_pages: &mut u32,
    ) -> (Document, LinkReport) {
        if page.error.is_some() {
            return (Document::default(), LinkReport::default());
        }
        if page.body.has_links() {
            let dir = page.file.as_deref().map(parent).unwrap_or("");
            let resolver = PageResolver::new(index, dir, self.workspace, same_workspace);
            let document = self.source.convert(page, &resolver, notes);
            let document = normalise::normalise(document, &resolver, notes);
            let (report, resolver_notes) = resolver.finish();
            notes.merge(&resolver_notes);
            return (document, report);
        }
        if page.kind == PageKind::Section {
            *section_pages += 1;
        }
        let document = if page.has_content() {
            self.source.convert(page, &super::DefaultResolver, notes)
        } else {
            Document::default()
        };
        let document = normalise::normalise(document, &super::DefaultResolver, notes);
        (document, LinkReport::default())
    }

    /// Take the pages the admin unticked out of the tree, with everything
    /// under them, before it is numbered. Returns the files they stood for
    /// (their Markdown and a database's CSV), which are left out: a link to
    /// one is kept as text, as for a file a pattern leaves out.
    fn leave_out_pages(
        &mut self,
        pages: &mut [SourcePage],
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
        top.retain(|(page, _)| !drop_if_excluded(pages, excluded, *page, &mut trail, &mut gone));
        for page in gone {
            for key in &pages[page].entries {
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

    /// The top-level pages, each with its collection, after applying the
    /// collection mode.
    fn collections(
        &self,
        mode: CollectionMode,
        pages: &mut [SourcePage],
        roots: Vec<usize>,
        site_name: Option<String>,
        source_collections: Option<Vec<PlannedCollection>>,
    ) -> (Vec<PlannedCollection>, Vec<(usize, Option<usize>)>) {
        let default_name = {
            self.options
                .collection_name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .or(site_name)
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
        match (mode, source_collections) {
            (CollectionMode::Existing, _) => {
                (Vec::new(), roots.iter().map(|r| (*r, None)).collect())
            }
            (CollectionMode::AsSource, Some(collections)) => {
                // Collections in their order, pages without one after.
                let mut top: Vec<(usize, Option<usize>)> =
                    roots.iter().map(|r| (*r, pages[*r].collection)).collect();
                top.sort_by_key(|(_, c)| (c.is_none(), *c));
                (collections, top)
            }
            (CollectionMode::PerTopLevelFolder, _) => {
                let mut collections = Vec::new();
                let mut top = Vec::new();
                let mut loose: Option<usize> = None;
                for root in roots {
                    if pages[root].group {
                        collections.push(new_collection(pages[root].title.clone()));
                        let collection = collections.len() - 1;
                        let children = pages[root].children.clone();
                        if pages[root].has_content() {
                            // The folder's own page leads its collection.
                            pages[root].children.clear();
                            top.push((root, Some(collection)));
                        }
                        for child in children {
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
            (CollectionMode::New | CollectionMode::AsSource, _) => (
                vec![new_collection(default_name)],
                roots.iter().map(|r| (*r, Some(0))).collect(),
            ),
        }
    }
}

/// Whether `page` is excluded; if so, it and everything under it go in
/// `gone`, and otherwise its excluded children leave it.
fn drop_if_excluded(
    pages: &mut [SourcePage],
    excluded: &ExcludedPages,
    page: usize,
    trail: &mut Vec<String>,
    gone: &mut Vec<usize>,
) -> bool {
    trail.push(pages[page].title.clone());
    let hit = excluded.matches(
        pages[page].file.as_deref(),
        pages[page].folder.as_deref(),
        &pages[page].key,
        trail,
    );
    if hit {
        let mut stack = vec![page];
        while let Some(n) = stack.pop() {
            gone.push(n);
            stack.extend(pages[n].children.iter().copied());
        }
    } else {
        let children = pages[page].children.clone();
        let kept: Vec<usize> = children
            .into_iter()
            .filter(|c| !drop_if_excluded(pages, excluded, *c, trail, gone))
            .collect();
        pages[page].children = kept;
    }
    trail.pop();
    hit
}

/// The collections a top-level page in `top` goes in, in their order, with
/// `top` pointed at their new places.
fn keep_used_collections(
    collections: Vec<PlannedCollection>,
    top: &mut [(usize, Option<usize>)],
) -> Vec<PlannedCollection> {
    let used: HashSet<usize> = top.iter().filter_map(|(_, c)| *c).collect();
    let mut moved_to: HashMap<usize, usize> = HashMap::new();
    let mut kept = Vec::with_capacity(used.len());
    for (i, collection) in collections.into_iter().enumerate() {
        if used.contains(&i) {
            moved_to.insert(i, kept.len());
            kept.push(collection);
        }
    }
    for (_, collection) in top.iter_mut() {
        *collection = collection.and_then(|c| moved_to.get(&c).copied());
    }
    kept
}

fn walk(
    pages: &[SourcePage],
    page: usize,
    parent: Option<usize>,
    position: u32,
    collection: Option<usize>,
    out: &mut Vec<(usize, Option<usize>, u32, Option<usize>)>,
) {
    out.push((page, parent, position, collection));
    for (i, child) in pages[page].children.iter().enumerate() {
        walk(pages, *child, Some(page), i as u32, None, out);
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
