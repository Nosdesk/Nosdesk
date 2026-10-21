//! Where each of an archive's files lands in the target workspace, which
//! files stay out and why, and putting them there.
//!
//! Most files keep their key. An attachment is served by its exact
//! `attachments.url` (`repository::file_access`), so its key and its url
//! come over together as they are. Two kinds of file name their owner in
//! their key instead, and move with it: a notes image
//! (`tickets/{id}/notes/`) follows its ticket, and an asset's media
//! (`assets/{id}/media/`, thumbnails included) its asset. Their keys are
//! rewritten by the same test that rewrites the references to them
//! ([`refs::file_owner`]: an editor's image `src`, a document's
//! `image.src`, `asset_media.url` and `thumbnail_url`), so a file lands
//! exactly where its references now point. A file whose owner isn't
//! imported stays out, as its references became text.
//!
//! Branding images keep their key (`branding/{name}`, under the
//! workspace's own prefix) and the settings that name them take the target
//! workspace's address ([`settle_branding`]).
//!
//! Every file goes through the upload validator first, with the rules of
//! the upload that made it: a file the app would refuse to take stays out.
//! A file that stays out is reported, never fatal, and so is a file a row
//! names that the archive doesn't carry.
//!
//! The restore runs after the rows commit and can run again: a key already
//! in place is left alone, so a second run puts only what the first
//! didn't.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::refs::{self, FilePath, Lookup};
use crate::db::DbConnection;
use crate::handlers::branding::{is_allowed_branding_filename, BRANDING_DIR};
use crate::repository::site_settings::{self, KeptBranding};
use crate::services::workspace_export::is_exported_file;
use crate::services::workspace_import::ArchiveContents;
use crate::utils::file_validation::{get_max_file_size, FileValidator};
use crate::utils::storage::{is_safe_storage_path, Storage, WorkspaceScopedStorage};

/// The largest notes image the upload takes.
const MAX_NOTES_IMAGE: usize = 10 * 1024 * 1024;
/// The largest branding image the upload takes.
const MAX_BRANDING_IMAGE: usize = 2 * 1024 * 1024;
/// What a branding upload may be (no SVG: branding renders on the public
/// sign-in page).
const BRANDING_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/x-icon",
    "image/vnd.microsoft.icon",
];

/// Why a file stays out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Its path could leave the workspace's storage.
    UnsafePath,
    /// The upload validator refuses it.
    Refused,
    /// The ticket or asset it belongs to isn't imported.
    NotImported,
    /// A row names it and the archive doesn't carry it.
    Missing,
}

/// A file the restore leaves out, by its path in the archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedFile {
    pub path: String,
    pub reason: SkipReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One file the restore puts.
#[derive(Debug, Clone)]
struct Entry {
    /// Index into `ArchiveContents::files`.
    index: usize,
    /// Its path in the archive, under `files/`.
    path: String,
    /// Its key in the target workspace's storage.
    key: String,
    content_type: String,
    /// Put even when the key is there: branding keys are fixed names the
    /// target may already use for its own images.
    overwrite: bool,
}

/// Where every archive file lands, and the ones that stay out.
#[derive(Debug, Clone, Default)]
pub struct FilePlan {
    entries: Vec<Entry>,
    pub skipped: Vec<SkippedFile>,
}

/// Which upload a file came from, for its validation rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    NotesImage,
    AssetMedia,
    Branding,
    Other,
}

impl FilePlan {
    /// Map every file in `contents` through `lookup` (the loaded rows' new
    /// keys, [`super::plan::Allocation::lookup`]).
    pub fn build(contents: &ArchiveContents, lookup: &dyn Lookup) -> Self {
        let mut plan = FilePlan::default();
        let mut carried: HashSet<&str> = HashSet::new();
        for (index, (name, bytes)) in contents.files.iter().enumerate() {
            let path = name.strip_prefix("files/").unwrap_or(name);
            carried.insert(path);
            if path.is_empty() || !is_safe_storage_path(path) {
                plan.skip(path, SkipReason::UnsafePath, None);
                continue;
            }
            let class = classify(path);
            let key = match class {
                Class::NotesImage | Class::AssetMedia => {
                    match refs::rewrite_file_path(&format!("/{path}"), lookup) {
                        FilePath::Kept(new) => new.trim_start_matches('/').to_string(),
                        FilePath::NotOwned => path.to_string(),
                        FilePath::Missing => {
                            plan.skip(path, SkipReason::NotImported, None);
                            continue;
                        }
                    }
                }
                Class::Branding | Class::Other => path.to_string(),
            };
            match validate(class, path, bytes) {
                Ok(content_type) => plan.entries.push(Entry {
                    index,
                    path: path.to_string(),
                    key,
                    content_type,
                    overwrite: class == Class::Branding,
                }),
                Err(detail) => plan.skip(path, SkipReason::Refused, Some(detail)),
            }
        }
        for path in named_files(contents) {
            if !carried.contains(path.as_str()) {
                plan.skip(&path, SkipReason::Missing, None);
            }
        }
        plan
    }

    fn skip(&mut self, path: &str, reason: SkipReason, detail: Option<String>) {
        self.skipped.push(SkippedFile {
            path: path.to_string(),
            reason,
            detail,
        });
    }

    /// Whether the restore puts the archive file at `path` (under `files/`).
    pub fn restores(&self, path: &str) -> bool {
        self.entries.iter().any(|e| e.path == path)
    }

    /// The target key of the archive file at `path`, when it is restored.
    pub fn key_of(&self, path: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.path == path)
            .map(|e| e.key.as_str())
    }

    /// How many files the restore puts.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// `path` (`tickets/{id}/notes/...`, `assets/{id}/media/...`,
/// `branding/...`, anything else) by the upload that made it.
fn classify(path: &str) -> Class {
    let mut parts = path.splitn(4, '/');
    let numeric = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("tickets"), Some(id), Some("notes"), Some(_)) if numeric(id) => Class::NotesImage,
        (Some("assets"), Some(id), Some("media"), Some(_)) if numeric(id) => Class::AssetMedia,
        (Some(BRANDING_DIR), Some(_), None, None) => Class::Branding,
        _ => Class::Other,
    }
}

/// The content type to store `bytes` under, or why the app would refuse
/// them: the upload validator for every file, plus the notes image and
/// branding uploads' own rules.
fn validate(class: Class, path: &str, bytes: &[u8]) -> Result<String, String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let max = match class {
        Class::NotesImage => MAX_NOTES_IMAGE.min(get_max_file_size()),
        Class::Branding => MAX_BRANDING_IMAGE,
        Class::AssetMedia | Class::Other => get_max_file_size(),
    };
    if bytes.len() > max {
        return Err(format!(
            "{} bytes is over the {max}-byte limit for this kind of file",
            bytes.len()
        ));
    }
    let detected = FileValidator::validate_file(bytes, Some(name)).map_err(|e| e.to_string())?;
    match class {
        Class::NotesImage if !detected.starts_with("image/") => {
            return Err(format!("a notes image must be an image, not {detected}"));
        }
        Class::Branding
            if !is_allowed_branding_filename(name)
                || !BRANDING_TYPES.contains(&detected.as_str()) =>
        {
            return Err(format!("not a branding image the app takes ({detected})"));
        }
        _ => {}
    }
    Ok(if detected == "application/octet-stream" {
        content_type_for(name).to_string()
    } else {
        detected
    })
}

/// Best-effort content type from a file extension, for files the
/// validator has no signature for.
fn content_type_for(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("txt") | Some("log") => "text/plain",
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        Some("md") => "text/markdown",
        Some("eml") => "message/rfc822",
        _ => "application/octet-stream",
    }
}

/// A stored file's key from the URL a row holds it by
/// (`/uploads/{key}` or `/api/files/{key}`, any query dropped).
fn stored_key(url: &str) -> Option<&str> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let key = path
        .strip_prefix("/uploads/")
        .or_else(|| path.strip_prefix("/api/files/"))?;
    (!key.is_empty()).then_some(key)
}

/// A branding image's file name and its URL's query (`?v=...`, or empty),
/// from `/uploads/branding/{workspace_uuid}/{name}` or a legacy
/// `/uploads/branding/{name}`.
fn branding_file(url: &str) -> Option<(&str, &str)> {
    let (path, query) = match url.find('?') {
        Some(i) => url.split_at(i),
        None => (url, ""),
    };
    let rest = path.strip_prefix(&format!("/uploads/{BRANDING_DIR}/"))?;
    let name = match rest.split_once('/') {
        Some((workspace, name)) => {
            Uuid::parse_str(workspace).ok()?;
            name
        }
        None => rest,
    };
    (!name.contains('/') && is_allowed_branding_filename(name)).then_some((name, query))
}

/// The archive's `site_settings` row.
fn archive_settings(contents: &ArchiveContents) -> Option<&serde_json::Map<String, Value>> {
    contents
        .tables
        .get("site_settings")
        .and_then(|rows| rows.first())
        .and_then(Value::as_object)
}

/// The branding URLs a settings row holds: the three images and the two
/// logos' email copies.
fn branding_urls(settings: &serde_json::Map<String, Value>) -> Vec<&str> {
    let mut urls: Vec<&str> = ["logo_url", "logo_light_url", "favicon_url"]
        .iter()
        .filter_map(|c| settings.get(*c).and_then(Value::as_str))
        .collect();
    for copy in ["email_logo", "email_logo_light"] {
        if let Some(url) = settings
            .get(copy)
            .and_then(|v| v.get("url"))
            .and_then(Value::as_str)
        {
            urls.push(url);
        }
    }
    urls
}

/// The files (archive paths under `files/`) the archive's rows name:
/// attachments, asset media and their thumbnails, and branding images.
/// Staged uploads (`temp/`) never travel, so they aren't counted.
fn named_files(contents: &ArchiveContents) -> BTreeSet<String> {
    let mut named = BTreeSet::new();
    let rows = |table: &str| contents.tables.get(table).into_iter().flatten();
    for (table, columns) in [
        ("attachments", &["url"][..]),
        ("asset_media", &["url", "thumbnail_url"][..]),
    ] {
        for row in rows(table) {
            for column in columns {
                if let Some(key) = row
                    .get(*column)
                    .and_then(Value::as_str)
                    .and_then(stored_key)
                    .filter(|k| is_exported_file(k))
                {
                    named.insert(key.to_string());
                }
            }
        }
    }
    if let Some(settings) = archive_settings(contents) {
        for url in branding_urls(settings) {
            if let Some((name, _)) = branding_file(url) {
                named.insert(format!("{BRANDING_DIR}/{name}"));
            }
        }
    }
    named
}

// ---------------------------------------------------------------------------
// Branding
// ---------------------------------------------------------------------------

/// The branding the target ends with ([`settle_branding`]).
pub fn branding(
    contents: &ArchiveContents,
    plan: &FilePlan,
    workspace_uuid: Uuid,
    kept: Option<&KeptBranding>,
) -> KeptBranding {
    let settings = archive_settings(contents);
    let take = |url: Option<&str>| -> Option<String> {
        let (name, query) = branding_file(url?)?;
        plan.restores(&format!("{BRANDING_DIR}/{name}"))
            .then(|| format!("/uploads/{BRANDING_DIR}/{workspace_uuid}/{name}{query}"))
    };
    let text = |column: &str| settings.and_then(|s| s.get(column)).and_then(Value::as_str);
    let copy = |column: &str| -> Option<Value> {
        let mut copy = settings.and_then(|s| s.get(column))?.clone();
        let url = take(copy.get("url").and_then(Value::as_str))?;
        copy.as_object_mut()?
            .insert("url".into(), Value::String(url));
        Some(copy)
    };
    let mut out = KeptBranding {
        app_name: kept.and_then(|k| k.app_name.clone()),
        ..KeptBranding::default()
    };
    match take(text("logo_url")) {
        Some(url) => {
            out.logo_url = Some(url);
            // No copy restored: the scheduler makes one from the logo.
            out.email_logo = copy("email_logo");
        }
        None => {
            out.logo_url = kept.and_then(|k| k.logo_url.clone());
            out.email_logo = kept.and_then(|k| k.email_logo.clone());
        }
    }
    match take(text("logo_light_url")) {
        Some(url) => {
            out.logo_light_url = Some(url);
            out.email_logo_light = copy("email_logo_light");
        }
        None => {
            out.logo_light_url = kept.and_then(|k| k.logo_light_url.clone());
            out.email_logo_light = kept.and_then(|k| k.email_logo_light.clone());
        }
    }
    out.favicon_url =
        take(text("favicon_url")).or_else(|| kept.and_then(|k| k.favicon_url.clone()));
    out
}

/// Point the workspace's branding settings at its own images, after the
/// archive's rows load: each image the archive names and the restore puts
/// in place, at `/uploads/branding/{workspace_uuid}/{name}`; otherwise the
/// target's own (`kept`, read before the load replaced its settings), or
/// none. Never a URL into the source workspace, whose uuid the archive's
/// settings name.
pub fn settle_branding(
    conn: &mut DbConnection,
    workspace_id: i32,
    workspace_uuid: Uuid,
    contents: &ArchiveContents,
    plan: &FilePlan,
    kept: Option<&KeptBranding>,
) -> diesel::QueryResult<()> {
    if kept.is_none() && archive_settings(contents).is_none() {
        return Ok(());
    }
    let settled = branding(contents, plan, workspace_uuid, kept);
    site_settings::restore_kept_branding(conn, workspace_id, &settled)
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

/// What [`restore`] did.
#[derive(Debug, Default, Serialize)]
pub struct FilesRestored {
    /// Files put.
    pub restored: i64,
    /// Files already in place from an earlier run.
    pub present: i64,
    /// Files left out, and why.
    pub skipped: Vec<SkippedFile>,
    /// The key whose put failed, when one did; the restore stops there and
    /// a second run picks up from it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
}

/// Put the plan's files into the workspace's storage. A key already there
/// is left alone (a branding image only when it holds the same bytes), so
/// running it again with the same archive and plan puts only what an
/// earlier run didn't.
pub async fn restore(
    storage: Arc<dyn Storage>,
    workspace_id: i32,
    contents: &ArchiveContents,
    plan: &FilePlan,
) -> FilesRestored {
    let scoped = WorkspaceScopedStorage::arc(storage, workspace_id);
    let mut out = FilesRestored {
        skipped: plan.skipped.clone(),
        ..FilesRestored::default()
    };
    // Unlisted, every file is put again: puts overwrite.
    let present: HashSet<String> = match scoped.list_prefix("").await {
        Ok(paths) => paths.into_iter().collect(),
        Err(e) => {
            tracing::warn!(error = ?e, "workspace import: could not list the workspace's files");
            HashSet::new()
        }
    };
    for entry in &plan.entries {
        let Some((_, bytes)) = contents.files.get(entry.index) else {
            continue;
        };
        if present.contains(&entry.key) {
            let same = !entry.overwrite
                || scoped
                    .get_file(&entry.key)
                    .await
                    .is_ok_and(|held| held == *bytes);
            if same {
                out.present += 1;
                continue;
            }
        }
        if let Err(e) = scoped
            .put_file(bytes, &entry.key, &entry.content_type)
            .await
        {
            tracing::error!(error = ?e, "workspace import: file restore failed part way");
            out.failed = Some(entry.key.clone());
            break;
        }
        out.restored += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::workspace_archive::refs::Entity;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0";
    const SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";

    /// Ticket 7 is now 70 and asset 3 is now 30; nothing else came.
    struct Moved;

    impl Lookup for Moved {
        fn user(&self, uuid: &str) -> Option<String> {
            Some(uuid.to_string())
        }
        fn key(&self, entity: Entity, key: &str) -> Option<String> {
            match (entity, key) {
                (Entity::Ticket, "7") => Some("70".into()),
                (Entity::Asset, "3") => Some("30".into()),
                _ => None,
            }
        }
    }

    fn contents(files: &[(&str, &[u8])], tables: Value) -> ArchiveContents {
        let manifest = serde_json::from_value(serde_json::json!({
            "workspace_export_format_version": 2,
            "nosdesk_version": "test",
            "schema_hash": "",
            "created_at": "",
            "workspace_id": 1,
            "workspace_slug": "s",
            "workspace_uuid": Uuid::nil(),
            "member_user_uuids": [],
            "tables": {},
            "files": {"total_count": 0, "total_size_bytes": 0},
        }))
        .expect("manifest");
        ArchiveContents {
            manifest,
            tables: serde_json::from_value(tables).expect("tables"),
            files: files
                .iter()
                .map(|(p, b)| (format!("files/{p}"), b.to_vec()))
                .collect(),
        }
    }

    fn reasons(plan: &FilePlan) -> Vec<(&str, SkipReason)> {
        plan.skipped
            .iter()
            .map(|s| (s.path.as_str(), s.reason))
            .collect()
    }

    #[test]
    fn owned_files_move_with_their_owner_and_the_rest_keep_their_key() {
        let source = Uuid::new_v4();
        let archive = contents(
            &[
                ("tickets/7/0198_invoice.pdf", b"%PDF-1.7\n"),
                ("tickets/7/notes/0198_shot.png", PNG),
                ("tickets/8/notes/0198_gone.png", PNG),
                ("assets/3/media/0198_photo.png", PNG),
                ("assets/3/media/thumb/0198.webp", b"RIFF\0\0\0\0WEBPVP8 "),
                ("branding/logo.png", PNG),
                ("../escape.png", PNG),
            ],
            serde_json::json!({
                "attachments": [
                    {"url": "/uploads/tickets/7/0198_invoice.pdf"},
                    {"url": "/uploads/tickets/7/0198_lost.pdf"},
                    {"url": "/uploads/temp/0198_draft.png"},
                ],
                "site_settings": [{"logo_url": format!("/uploads/branding/{source}/logo.png?v=9")}],
            }),
        );
        let plan = FilePlan::build(&archive, &Moved);
        assert_eq!(
            plan.key_of("tickets/7/0198_invoice.pdf"),
            Some("tickets/7/0198_invoice.pdf")
        );
        assert_eq!(
            plan.key_of("tickets/7/notes/0198_shot.png"),
            Some("tickets/70/notes/0198_shot.png")
        );
        assert_eq!(
            plan.key_of("assets/3/media/0198_photo.png"),
            Some("assets/30/media/0198_photo.png")
        );
        assert_eq!(
            plan.key_of("assets/3/media/thumb/0198.webp"),
            Some("assets/30/media/thumb/0198.webp")
        );
        assert_eq!(plan.key_of("branding/logo.png"), Some("branding/logo.png"));
        assert_eq!(
            reasons(&plan),
            vec![
                ("tickets/8/notes/0198_gone.png", SkipReason::NotImported),
                ("../escape.png", SkipReason::UnsafePath),
                ("tickets/7/0198_lost.pdf", SkipReason::Missing),
            ]
        );

        // The settings take the target's address for the restored logo.
        let target = Uuid::new_v4();
        let settled = branding(&archive, &plan, target, None);
        assert_eq!(
            settled.logo_url,
            Some(format!("/uploads/branding/{target}/logo.png?v=9"))
        );
        assert_eq!(settled.favicon_url, None);
    }

    /// A file the app's upload refuses stays out, by the upload's own
    /// rules: SVG anywhere, a notes image that isn't an image, a branding
    /// image of a kind branding doesn't take.
    #[test]
    fn what_the_upload_refuses_stays_out() {
        let archive = contents(
            &[
                ("tickets/7/0198_vector.svg", SVG),
                ("tickets/7/0198_renamed.png", SVG),
                ("tickets/7/notes/0198_notes.pdf", b"%PDF-1.7\n"),
                ("branding/favicon.ico", b"%PDF-1.7\n"),
                ("tickets/7/0198_fine.txt", b"hello"),
            ],
            serde_json::json!({}),
        );
        let plan = FilePlan::build(&archive, &Moved);
        let refused: Vec<&str> = plan
            .skipped
            .iter()
            .filter(|s| s.reason == SkipReason::Refused)
            .map(|s| s.path.as_str())
            .collect();
        assert_eq!(
            refused,
            vec![
                "tickets/7/0198_vector.svg",
                "tickets/7/0198_renamed.png",
                "tickets/7/notes/0198_notes.pdf",
                "branding/favicon.ico",
            ]
        );
        assert_eq!(plan.len(), 1);
    }

    /// An image the archive names but doesn't restore leaves the target's
    /// own in its place; a legacy flat URL takes the target's address.
    #[test]
    fn branding_the_archive_cannot_restore_keeps_the_targets() {
        let target = Uuid::new_v4();
        let archive = contents(
            &[("branding/logo_light_1699999999.png", PNG)],
            serde_json::json!({"site_settings": [{
                "logo_url": format!("/uploads/branding/{}/logo.png?v=1", Uuid::new_v4()),
                "logo_light_url": "/uploads/branding/logo_light_1699999999.png",
                "email_logo": {"url": format!("/uploads/branding/{}/email_logo_1.png", Uuid::new_v4())},
            }]}),
        );
        let plan = FilePlan::build(&archive, &Moved);
        let kept = KeptBranding {
            app_name: Some("Portal".into()),
            logo_url: Some(format!("/uploads/branding/{target}/logo.webp?v=2")),
            favicon_url: Some(format!("/uploads/branding/{target}/favicon.ico?v=2")),
            ..KeptBranding::default()
        };
        let settled = branding(&archive, &plan, target, Some(&kept));
        assert_eq!(settled.logo_url, kept.logo_url, "logo.png isn't carried");
        assert_eq!(
            settled.logo_light_url,
            Some(format!(
                "/uploads/branding/{target}/logo_light_1699999999.png"
            ))
        );
        assert_eq!(settled.favicon_url, kept.favicon_url);
        assert_eq!(settled.app_name.as_deref(), Some("Portal"));
        assert_eq!(
            reasons(&plan),
            vec![
                ("branding/email_logo_1.png", SkipReason::Missing),
                ("branding/logo.png", SkipReason::Missing),
            ]
        );
    }
}
