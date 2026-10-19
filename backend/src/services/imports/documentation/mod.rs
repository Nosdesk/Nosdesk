//! Documentation imports: a zip of Markdown (or a few loose `.md` files)
//! becomes linked pages, through the importer's job model.
//!
//! 1. Upload streams the file to `imports/{job}/upload.zip` (200 MB cap,
//!    never whole in memory), reads it once under the default options and
//!    keeps the tree as Nosdesk will show it in the job's `source`.
//! 2. The check ([`check`]) runs in the background (`checking`): the full
//!    conversion under the admin's options, archive and `data:` images read
//!    and validated, web images downloaded through `utils::safe_http`
//!    ([`web`]) to `imports/{job}/web/`, pages imported before found, each
//!    page's preview written to `imports/{job}/preview/{index}.json`. The
//!    job lands in `dry_run_done` with the summary and every page's result.
//! 3. The commit ([`commit`]) runs in the background (`committing`): images
//!    stored under each new page, then one transaction writing collections
//!    and pages, their bodies, embeds and first revision, and the job's
//!    `done`. Nothing is written to documentation tables before it.
//!
//! A background run marks the job alive every [`HEARTBEAT`]; one silent for
//! [`STALLED_AFTER`] died with its process and is put back
//! ([`reconcile`]) on the next request for the job or by the daily prune.

pub mod background;
pub mod check;
pub mod commit;
pub mod images;
pub mod preview;
pub mod reconcile;
pub mod remove;
pub mod report;
pub mod web;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::extractors::TenantConn;
use crate::models::ImportJob;
use crate::repository::documentation_imports as docs_repo;
use crate::services::docs_markdown::links::{LinkReport, Workspace};
use crate::services::docs_markdown::profiles::ProfileKind;
use crate::services::docs_markdown::tree::{
    self, CollectionMode, ImportOptions as TreeOptions, Plan, StatusMode,
};
use crate::services::docs_markdown::{NoteCode, Notes};

/// `import_jobs.job_type` of a documentation import.
pub const JOB_TYPE: &str = "documentation";

/// Largest upload this route takes, streamed.
pub const MAX_UPLOAD_BYTES: u64 = 200 * 1024 * 1024;

/// How often a background run marks the job alive.
pub const HEARTBEAT: Duration = Duration::from_secs(30);

/// A background run silent this long died with its process.
pub const STALLED_AFTER: Duration = Duration::from_secs(5 * 60);

/// Longest the conversion may take at upload and at the check.
pub const CONVERT_BUDGET: Duration = Duration::from_secs(60);

/// The upload as stored.
pub fn upload_key(job_id: Uuid) -> String {
    format!("imports/{job_id}/upload.zip")
}

/// A downloaded web image, named by its URL's hash.
pub fn web_key(job_id: Uuid, url: &str, extension: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = hex::encode(Sha256::digest(url.as_bytes()));
    format!("imports/{job_id}/web/{}.{extension}", &hash[..32])
}

/// One page's preview.
pub fn preview_key(job_id: Uuid, index: usize) -> String {
    format!("imports/{job_id}/preview/{index}.json")
}

/// Images linked from the web.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebImages {
    /// Downloaded at the check and stored with the page: the editor shows
    /// only images the workspace serves.
    #[default]
    Download,
    /// The URL is kept as the image's source.
    Links,
}

/// Pages found in the workspace already.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Duplicates {
    #[default]
    Skip,
    /// Imported again as new pages.
    Copy,
}

/// What the admin chose at Pages and Review.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DocsOptions {
    /// Forces a source profile; detection's otherwise.
    pub profile: Option<ProfileKind>,
    /// The profile's default when unset.
    pub collections: Option<CollectionMode>,
    /// A new collection's name; the source's or the file's otherwise.
    pub collection_name: Option<String>,
    /// The collection pages go in, for [`CollectionMode::Existing`].
    pub collection_id: Option<i32>,
    /// The page they go under in it, if any.
    pub under_page_id: Option<i32>,
    pub status: StatusMode,
    pub web_images: WebImages,
    /// Archive paths left out, as globs (`**/archive/**`).
    pub leave_out: Vec<String>,
    pub duplicates: Duplicates,
}

impl DocsOptions {
    /// What's wrong with the options, if anything.
    pub fn invalid(&self) -> Option<String> {
        let existing = self.collections == Some(CollectionMode::Existing);
        if existing && self.collection_id.is_none() {
            return Some("choose the collection to import into".to_string());
        }
        if !existing && (self.collection_id.is_some() || self.under_page_id.is_some()) {
            return Some(
                "a collection or page to import under goes with an existing collection".to_string(),
            );
        }
        if let Some(name) = &self.collection_name {
            if name.trim().is_empty() || name.chars().count() > 255 {
                return Some("a collection name is 1 to 255 characters".to_string());
            }
        }
        if self.leave_out.len() > 100 || self.leave_out.iter().any(|g| g.len() > 500) {
            return Some(
                "at most 100 patterns to leave out, each up to 500 characters".to_string(),
            );
        }
        None
    }

    /// The converter's options.
    pub fn tree_options(&self, archive_name: &str) -> TreeOptions {
        TreeOptions {
            profile: self.profile,
            collections: self.collections,
            collection_name: self
                .collection_name
                .as_deref()
                .map(str::trim)
                .map(str::to_string),
            archive_name: Some(archive_name.to_string()),
            status: self.status,
            leave_out: self.leave_out.clone(),
        }
    }

    pub fn from_job(job: &ImportJob) -> Self {
        job.options
            .clone()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }
}

/// A page of the tree as Pages shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreePage {
    pub index: usize,
    pub title: String,
    /// The Markdown file, or the folder a section page stands for.
    pub path: String,
    pub file: Option<String>,
    pub folder: Option<String>,
    pub parent: Option<usize>,
    pub order: u32,
    /// The new collection a top-level page goes in, by index.
    pub collection: Option<usize>,
}

/// The tree an upload or a profile change reads: what Pages shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreeView {
    pub profile: ProfileKind,
    /// What detection said, when a chosen profile replaced it.
    pub detected: Option<ProfileKind>,
    /// The collections the import would create, by name.
    pub collections: Vec<String>,
    pub pages: Vec<TreePage>,
    pub section_pages: u32,
    /// Images in the archive the pages show.
    pub images: u32,
    /// Images linked from the web.
    pub web_images: u32,
    /// Files left out (by a pattern, or neither a page nor an image).
    pub left_out: usize,
}

impl TreeView {
    pub fn of(plan: &Plan, documents: &[crate::services::docs_markdown::ir::Document]) -> Self {
        let mut archive_images = std::collections::BTreeSet::new();
        let mut web_images = std::collections::BTreeSet::new();
        for page in &plan.pages {
            archive_images.extend(page.links.images.iter().cloned());
        }
        for document in documents {
            images::for_each_image(document, &mut |image| {
                if images::is_web(&image.src) {
                    web_images.insert(image.src.clone());
                }
            });
        }
        TreeView {
            profile: plan.profile,
            detected: plan.detected,
            collections: plan.collections.iter().map(|c| c.name.clone()).collect(),
            pages: plan
                .pages
                .iter()
                .map(|p| TreePage {
                    index: p.index,
                    title: p.title.clone(),
                    path: p
                        .source
                        .clone()
                        .or_else(|| p.folder.clone())
                        .unwrap_or_default(),
                    file: p.source.clone(),
                    folder: p.folder.clone(),
                    parent: p.parent,
                    order: p.order,
                    collection: p.collection,
                })
                .collect(),
            section_pages: plan.section_pages,
            images: archive_images.len() as u32,
            web_images: web_images.len() as u32,
            left_out: plan.left_out.len(),
        }
    }
}

/// `import_jobs.source` of a documentation job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocsSource {
    /// Always [`JOB_TYPE`]: tells this shape from a spreadsheet's.
    pub kind: String,
    /// The upload's size in bytes.
    pub size: u64,
    /// Files in the archive.
    pub files: usize,
    pub tree: TreeView,
}

impl DocsSource {
    pub fn from_job(job: &ImportJob) -> Option<Self> {
        serde_json::from_value(job.source.clone()?).ok()
    }
}

/// A page's outcome at the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageOutcome {
    New,
    /// In the workspace already, and the admin chose to skip those.
    Skipped,
    /// Its file couldn't be read; the page is imported empty.
    Error,
}

/// How a page was found in the workspace already.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FoundBy {
    /// Its Nosdesk uuid names a page here.
    Uuid,
    /// An earlier import brought the same file into the same collection.
    Source,
}

/// The page in the workspace an imported page matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Duplicate {
    pub page_id: i32,
    pub page_uuid: Uuid,
    pub by: FoundBy,
}

/// Where an image comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageKind {
    /// A file in the archive.
    Archive,
    /// Written into the page as a `data:` URI.
    Data,
    /// Linked from the web.
    Web,
    /// Already stored in this workspace (a Nosdesk export of it).
    Workspace,
}

/// Why an image isn't imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageProblem {
    /// Missing from the archive, or damaged.
    Unreadable,
    /// Over 10 MB.
    TooLarge,
    NotAnImage,
    /// SVG can carry scripts.
    Svg,
    /// The address is on a private network, or isn't http or https.
    RefusedAddress,
    /// The server took longer than 10 seconds.
    TimedOut,
    /// The server answered with an error, or couldn't be reached.
    FetchFailed,
    /// Past the 500 web images one import downloads.
    OverLimit,
}

impl ImageProblem {
    pub fn note(self) -> NoteCode {
        match self {
            ImageProblem::Svg => NoteCode::SvgImagesRefused,
            ImageProblem::RefusedAddress
            | ImageProblem::TimedOut
            | ImageProblem::FetchFailed
            | ImageProblem::OverLimit => NoteCode::WebImagesNotFetched,
            ImageProblem::Unreadable | ImageProblem::TooLarge | ImageProblem::NotAnImage => {
                NoteCode::ImagesUnreadable
            }
        }
    }

    /// The reason as the conversion report words it.
    pub fn text(self) -> &'static str {
        match self {
            ImageProblem::Unreadable => "couldn't be read",
            ImageProblem::TooLarge => "is larger than 10 MB",
            ImageProblem::NotAnImage => "isn't an image",
            ImageProblem::Svg => "SVG images can't be imported",
            ImageProblem::RefusedAddress => "its address can't be downloaded from",
            ImageProblem::TimedOut => "took too long to download",
            ImageProblem::FetchFailed => "couldn't be downloaded",
            ImageProblem::OverLimit => "is past the 500 web images one import downloads",
        }
    }
}

/// An image a page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageImage {
    /// Where it comes from as the page wrote it: the archive path, the URL,
    /// or `data:image/png` for an image written into the page.
    pub source: String,
    pub kind: ImageKind,
    /// The file name it is stored under.
    pub name: String,
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<ImageProblem>,
}

/// One page's result at the check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageRow {
    pub index: usize,
    /// The Markdown file, or the folder a section page stands for.
    pub path: String,
    pub title: String,
    pub parent: Option<usize>,
    pub outcome: PageOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate: Option<Duplicate>,
    /// The Nosdesk uuid the file carries, for finding it again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_uuid: Option<String>,
    pub notes: Notes,
    pub links: LinkReport,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<PageImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl PageRow {
    pub fn has_notes(&self) -> bool {
        !self.notes.is_empty() || self.error.is_some()
    }
}

/// A web image as the check left it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebImage {
    /// Where it is kept, under `imports/{job}/web/`.
    pub key: Option<String>,
    pub name: String,
    pub size: Option<u64>,
    pub content_type: Option<String>,
    pub problem: Option<ImageProblem>,
}

/// `import_jobs.row_results` of a documentation job.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DocsRows {
    pub kind: String,
    pub pages: Vec<PageRow>,
    /// Every web image the pages link, by URL.
    #[serde(default)]
    pub web_images: BTreeMap<String, WebImage>,
}

impl DocsRows {
    pub fn from_value(value: Option<serde_json::Value>) -> Option<Self> {
        serde_json::from_value(value?).ok()
    }
}

/// One note code with the pages it touched, for Review's lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteLine {
    pub code: NoteCode,
    pub count: u32,
    pub pages: u32,
}

/// Where the pages go, as the check settled it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    pub mode: CollectionMode,
    /// The collections the import creates.
    pub new_collections: Vec<String>,
    /// The existing collection, with its name.
    pub collection_id: Option<i32>,
    pub collection_name: Option<String>,
    pub under_page_id: Option<i32>,
}

/// `import_jobs.summary` of a checked documentation job. Carries the
/// spreadsheet importer's count keys, which Recent imports reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocsSummary {
    pub kind: String,
    pub row_count: usize,
    pub would_create: usize,
    pub would_update: usize,
    pub unchanged: usize,
    /// Skipped pages, for Recent imports.
    pub rows_with_errors: usize,
    pub skipped: usize,
    /// Pages found in the workspace already, skipped or not.
    pub duplicates: usize,
    /// Pages whose file couldn't be read, imported empty.
    pub errors: usize,
    /// Images that will be stored.
    pub images: usize,
    /// Images left out, with the reason on their page.
    pub image_problems: usize,
    pub web_images: usize,
    pub links_rewritten: u32,
    pub notes: Vec<NoteLine>,
    pub section_pages: u32,
    pub left_out: usize,
    pub front_matter_not_imported: BTreeMap<String, u32>,
    pub profile: ProfileKind,
    pub detected: Option<ProfileKind>,
    pub destination: Destination,
    pub status: StatusMode,
    pub duplicates_choice: Duplicates,
    pub web_images_choice: WebImages,
}

impl DocsSummary {
    pub fn from_job(job: &ImportJob) -> Option<Self> {
        serde_json::from_value(job.summary.clone()?).ok()
    }
}

/// The summary the prune keeps: a documentation summary holds counts and
/// names only, so all of it.
pub fn pruned_summary(summary: Option<serde_json::Value>) -> Option<serde_json::Value> {
    summary
}

/// What the commit recorded: before its transaction, every page uuid it
/// minted and every storage key it may write (`pending`); after, every page
/// it wrote. Kept for the life of the workspace: Remove imported pages and
/// the next import's duplicate check read it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitRecord {
    pub kind: String,
    /// The commit run that wrote it.
    pub run: Uuid,
    pub pending: bool,
    pub page_uuids: Vec<Uuid>,
    pub storage_keys: Vec<String>,
    #[serde(default)]
    pub pages: Vec<RecordedPage>,
    /// Collections the import created.
    #[serde(default)]
    pub collections_created: Vec<i32>,
}

impl CommitRecord {
    pub fn from_value(value: Option<serde_json::Value>) -> Option<Self> {
        serde_json::from_value(value?).ok()
    }
}

/// A page the commit wrote, as it wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedPage {
    pub id: i32,
    pub uuid: Uuid,
    pub index: usize,
    /// The archive path it came from.
    pub source: String,
    pub collection_id: Option<i32>,
    /// `updated_at` after the commit wrote its body, as the page row
    /// holds it; once Publish all published the page, the one it left.
    pub updated_at: chrono::NaiveDateTime,
    /// SHA-256 of the `yjs_document` the commit wrote, hex.
    pub hash: String,
}

/// The tree's lookups against the workspace being imported into: whether a
/// mention, ticket link, embed or image in a Nosdesk export of this
/// workspace still points at something. Asked only for such archives, one
/// short read per name, cached.
pub struct TenantWorkspace {
    tc: RefCell<TenantConn>,
    workspace_uuid: Uuid,
    cache: RefCell<HashMap<String, bool>>,
}

impl TenantWorkspace {
    pub fn new(tc: TenantConn, workspace_uuid: Uuid) -> Self {
        Self {
            tc: RefCell::new(tc),
            workspace_uuid,
            cache: RefCell::new(HashMap::new()),
        }
    }

    fn ask(
        &self,
        key: String,
        read: impl FnOnce(&mut crate::db::DbConnection) -> diesel::QueryResult<bool>,
    ) -> bool {
        if let Some(found) = self.cache.borrow().get(&key) {
            return *found;
        }
        let found = match self.tc.borrow_mut().run(read) {
            Ok(found) => found,
            Err(e) => {
                tracing::warn!(error = ?e, "documentation import lookup failed");
                false
            }
        };
        self.cache.borrow_mut().insert(key, found);
        found
    }
}

impl Workspace for TenantWorkspace {
    fn is_this_workspace(&self, workspace_uuid: &str) -> bool {
        Uuid::parse_str(workspace_uuid.trim()).is_ok_and(|u| u == self.workspace_uuid)
    }

    fn user_exists(&self, uuid: &str) -> bool {
        let Ok(uuid) = Uuid::parse_str(uuid.trim()) else {
            return false;
        };
        self.ask(format!("user:{uuid}"), |conn| {
            docs_repo::member_exists(conn, uuid)
        })
    }

    fn ticket_exists(&self, ticket_id: &str) -> bool {
        let Ok(id) = ticket_id.trim().parse::<i32>() else {
            return false;
        };
        self.ask(format!("ticket:{id}"), |conn| {
            docs_repo::ticket_exists(conn, id)
        })
    }

    fn page_exists(&self, uuid: &str) -> bool {
        let Ok(uuid) = Uuid::parse_str(uuid.trim()) else {
            return false;
        };
        self.ask(format!("page:{uuid}"), |conn| {
            Ok(!docs_repo::pages_by_uuid(conn, &[uuid])?.is_empty())
        })
    }

    fn page_reference_exists(&self, reference: &str) -> bool {
        let reference = reference.trim().to_string();
        if reference.is_empty() || reference.len() > 255 {
            return false;
        }
        self.ask(format!("ref:{reference}"), |conn| {
            docs_repo::page_reference_exists(conn, &reference)
        })
    }

    fn image_exists(&self, src: &str) -> bool {
        // The page it was stored with still exists; the file itself is read
        // (and copied to the new page) at the commit.
        images::workspace_image_key(src)
            .and_then(|key| images::page_of_workspace_key(&key))
            .is_some_and(|uuid| self.page_exists(&uuid.to_string()))
    }
}

/// Every note code with its count and the number of pages it touched.
pub fn note_lines(rows: &[PageRow]) -> Vec<NoteLine> {
    let mut lines: BTreeMap<NoteCode, (u32, u32)> = BTreeMap::new();
    for row in rows {
        for (code, count) in row.notes.iter() {
            let line = lines.entry(code).or_insert((0, 0));
            line.0 += count;
            line.1 += 1;
        }
    }
    lines
        .into_iter()
        .map(|(code, (count, pages))| NoteLine { code, count, pages })
        .collect()
}

/// A short message for a job a background run left: "Nothing was imported.
/// {reason}".
pub fn nothing_imported(reason: &str) -> String {
    format!("Nothing was imported. {reason}")
}

/// The pages of a plan in an order where every parent comes before its
/// children. The tree writes them that way already; this holds even if it
/// one day doesn't.
pub fn parents_first(plan: &Plan) -> Vec<usize> {
    let mut depth: Vec<usize> = vec![0; plan.pages.len()];
    for (i, page) in plan.pages.iter().enumerate() {
        let mut d = 0;
        let mut at = page.parent;
        while let Some(p) = at {
            d += 1;
            if d > plan.pages.len() {
                break;
            }
            at = plan.pages.get(p).and_then(|q| q.parent);
        }
        depth[i] = d;
    }
    let mut order: Vec<usize> = (0..plan.pages.len()).collect();
    order.sort_by_key(|i| (depth[*i], *i));
    order
}

/// The tree under the default options of a fresh upload.
pub fn default_tree(
    archive: &mut crate::services::uploaded_archive::SafeArchive,
    archive_name: &str,
    workspace: &dyn Workspace,
) -> Result<TreeView, crate::services::imports::refusal::Refused> {
    let built = tree::build(
        archive,
        &DocsOptions::default().tree_options(archive_name),
        workspace,
    )?;
    Ok(TreeView::of(&built.plan, &built.documents))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_read_back_and_refuse_what_cant_go_together() {
        let options: DocsOptions = serde_json::from_value(serde_json::json!({
            "collections": "existing",
            "collection_id": 4,
            "web_images": "links",
            "duplicates": "copy",
            "leave_out": ["**/archive/**"],
        }))
        .unwrap();
        assert_eq!(options.invalid(), None);
        assert_eq!(options.web_images, WebImages::Links);
        assert_eq!(options.status, StatusMode::Drafts);

        let missing = DocsOptions {
            collections: Some(CollectionMode::Existing),
            ..DocsOptions::default()
        };
        assert!(missing.invalid().is_some());
        let stray = DocsOptions {
            under_page_id: Some(3),
            ..DocsOptions::default()
        };
        assert!(stray.invalid().is_some());
        assert!(serde_json::from_value::<DocsOptions>(serde_json::json!({ "nope": 1 })).is_err());
    }

    #[test]
    fn keys_stay_under_the_job_prefix() {
        let job = Uuid::now_v7();
        let prefix = format!("imports/{job}/");
        for key in [
            upload_key(job),
            web_key(job, "https://example.com/a.png", "png"),
            preview_key(job, 7),
        ] {
            assert!(key.starts_with(&prefix), "{key}");
            assert!(crate::utils::storage::is_safe_storage_path(&key), "{key}");
        }
        assert_ne!(
            web_key(job, "https://example.com/a.png", "png"),
            web_key(job, "https://example.com/b.png", "png")
        );
    }
}
