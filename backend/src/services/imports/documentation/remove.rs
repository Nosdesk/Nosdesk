//! Remove imported pages and Publish all (design §4, History and undo).
//!
//! Both read the pages the commit recorded in `record_ids`
//! ([`CommitRecord`]), which is kept for the life of the workspace, and
//! hold each page to the untouched rule: it has the `updated_at` the
//! commit read back, its `yjs_document` still hashes to what the commit
//! wrote, it has no revision after the import's (the import wrote
//! revision 1 of a page it created), and it is in the collection the
//! import put it in.
//!
//! Remove moves every untouched page to Trash, where it can be restored,
//! unless a page added since sits under it or a page that is kept does
//! (it would be stranded under a page in Trash). A collection the import
//! created is deleted when it ends up empty, through the same path as
//! deleting a collection: its pages are moved to Trash first. It runs once,
//! within 30 days of the commit.
//!
//! Publish all publishes the untouched pages still in draft, a status
//! change only, and records their new `updated_at` as the one the rule
//! compares with, so Remove afterward still sees them untouched.
//!
//! Both run in the request: an import holds at most 2,000 pages (the
//! upload's cap), the rule is a handful of batched reads, and each page is
//! one update with its sync event, so the transaction stays short.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::result::Error as DieselError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{DocumentationPage, DocumentationPageUpdate, DocumentationStatus};
use crate::repository::import_removal as repo;
use crate::repository::{self, documentation_collections as collections};

use super::{CommitRecord, RecordedPage, JOB_TYPE};

/// Why a page the import created is kept. A kept page carries every
/// reason that applies, most telling first (the order of the variants);
/// the first is the one the page shows.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum KeptReason {
    /// Its content or details changed, or it has a newer revision:
    /// "Edited on 3 Nov by Sam". `by` is who made the newest revision, or
    /// who last changed its details when its content is as imported;
    /// `by_name` is filled when the result is read.
    Edited {
        at: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by_name: Option<String>,
    },
    /// Pages the import didn't create sit under it: "Has 2 pages added
    /// since".
    PagesAdded { count: i64 },
    /// It is no longer in the collection the import put it in: "Moved to
    /// another collection".
    Moved,
    /// Pages of the import that are kept sit under it, and moving it to
    /// Trash would strand them: "Has 2 kept pages under it".
    KeptPagesUnder { count: i64 },
}

/// A page the import created that stays, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptPage {
    pub id: i32,
    pub title: String,
    pub reasons: Vec<KeptReason>,
}

/// A collection the import created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionOutcome {
    pub id: i32,
    pub name: String,
    /// Pages left in it, outside Trash.
    #[serde(default)]
    pub pages: usize,
}

/// What becomes of the collections the import created that still exist.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionsOutcome {
    /// Empty once the pages are in Trash: deleted.
    #[serde(default)]
    pub deleted: Vec<CollectionOutcome>,
    /// Still holding pages: kept.
    #[serde(default)]
    pub kept: Vec<CollectionOutcome>,
}

/// What removal would do now: the dialog's counts and kept list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemovalPlan {
    /// Pages the import created.
    pub created: usize,
    /// Of those, the ones that would move to Trash.
    pub deletable: usize,
    /// Of those, the ones deleted or in Trash already.
    pub gone: usize,
    pub kept: Vec<KeptPage>,
    pub collections: CollectionsOutcome,
    /// The ids `deletable` counts.
    #[serde(skip)]
    pub deletable_ids: Vec<i32>,
}

/// What removal did, as `import_jobs.remove_result` keeps it. `created`
/// and `deleted` are the keys Recent imports reads ("298 of 309
/// removed"); for pages, `deleted` counts the pages moved to Trash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveResult {
    /// Always [`JOB_TYPE`].
    pub kind: String,
    pub created: usize,
    pub deleted: usize,
    #[serde(default)]
    pub gone: usize,
    pub deleted_ids: Vec<i32>,
    pub kept: Vec<KeptPage>,
    pub collections: CollectionsOutcome,
}

impl RemoveResult {
    /// The people the kept list names, so a reader can fill in their names.
    pub fn editors(&self) -> Vec<Uuid> {
        editors_of(&self.kept)
    }

    /// Put the editors' names on the kept list.
    pub fn name_editors(&mut self, names: &HashMap<Uuid, String>) {
        name_editors(&mut self.kept, names);
    }
}

/// Put the editors' names on a kept list.
pub fn name_editors(kept: &mut [KeptPage], names: &HashMap<Uuid, String>) {
    for page in kept {
        for reason in &mut page.reasons {
            if let KeptReason::Edited { by, by_name, .. } = reason {
                *by_name = by.and_then(|u| names.get(&u).cloned());
            }
        }
    }
}

/// The people a kept list names.
pub fn editors_of(kept: &[KeptPage]) -> Vec<Uuid> {
    let mut out: Vec<Uuid> = kept
        .iter()
        .flat_map(|k| &k.reasons)
        .filter_map(|r| match r {
            KeptReason::Edited { by, .. } => *by,
            _ => None,
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// One recorded page held to the rule.
struct Checked {
    page: RecordedPage,
    /// `None` when the page is gone: deleted, or in Trash.
    state: Option<repo::PageState>,
    /// Reasons of its own, before kept pages under it are counted.
    reasons: Vec<KeptReason>,
}

/// The recorded pages, once each, in the order the commit wrote them.
fn recorded(record: &CommitRecord) -> Vec<RecordedPage> {
    let mut seen = HashSet::new();
    record
        .pages
        .iter()
        .filter(|p| seen.insert(p.id))
        .cloned()
        .collect()
}

fn hash_matches(state: &repo::PageState, page: &RecordedPage) -> bool {
    state
        .hash
        .as_deref()
        .is_some_and(|h| h.eq_ignore_ascii_case(&page.hash))
}

/// Whether the page's own content and place are as the import left them,
/// and when not, the reasons. `newest` is its newest revision.
fn own_reasons(
    page: &RecordedPage,
    state: &repo::PageState,
    newest: Option<&repo::NewestRevision>,
) -> Vec<KeptReason> {
    let mut reasons = Vec::new();
    let later_revision = newest.filter(|r| r.revision_number > 1);
    let content_same = hash_matches(state, page);
    if state.updated_at != page.updated_at || !content_same || later_revision.is_some() {
        let mut at = state.updated_at.and_utc();
        let mut by = None;
        if let Some(rev) = later_revision {
            at = at.max(rev.created_at);
            by = Some(rev.created_by);
        } else if content_same {
            // Only its details changed, which records who did it.
            by = Some(state.last_edited_by);
        }
        reasons.push(KeptReason::Edited {
            at,
            by,
            by_name: None,
        });
    }
    if state.collection_id != page.collection_id {
        reasons.push(KeptReason::Moved);
    }
    reasons
}

fn status_is(state: &repo::PageState, status: &str) -> bool {
    state.status == status
}

/// Every recorded page against the rule, with the pages under each.
fn check(
    conn: &mut DbConnection,
    record: &CommitRecord,
    lock: bool,
) -> Result<Vec<Checked>, DieselError> {
    let pages = recorded(record);
    let ids: Vec<i32> = pages.iter().map(|p| p.id).collect();
    let states: HashMap<i32, repo::PageState> = repo::page_states(conn, &ids, lock)?
        .into_iter()
        .map(|s| (s.id, s))
        .collect();
    let revisions = repo::newest_revisions(conn, &ids)?;
    let added = repo::children_outside(conn, &ids)?;
    Ok(pages
        .into_iter()
        .map(|page| {
            let state = states
                .get(&page.id)
                .filter(|s| !status_is(s, "deleted"))
                .cloned();
            let mut reasons = match &state {
                Some(s) => own_reasons(&page, s, revisions.get(&page.id)),
                None => Vec::new(),
            };
            if let Some(count) = added.get(&page.id).copied().filter(|n| *n > 0) {
                if state.is_some() {
                    reasons.push(KeptReason::PagesAdded { count });
                }
            }
            Checked {
                page,
                state,
                reasons,
            }
        })
        .collect())
}

/// Check every page the import created against the untouched rule, and
/// say which collections it created would be deleted. With `lock`, the
/// pages and collections are locked for the rest of the transaction.
pub fn plan(
    conn: &mut DbConnection,
    record: &CommitRecord,
    lock: bool,
) -> Result<RemovalPlan, DieselError> {
    let checked = check(conn, record, lock)?;
    let created = checked.len();
    let gone = checked.iter().filter(|c| c.state.is_none()).count();

    // A page with a kept page under it is kept too: count, for each page,
    // the kept pages anywhere below it among the import's.
    let parent_of: HashMap<i32, i32> = checked
        .iter()
        .filter_map(|c| Some((c.page.id, c.state.as_ref()?.parent_id?)))
        .collect();
    let mut kept_under: HashMap<i32, i64> = HashMap::new();
    for c in checked.iter().filter(|c| !c.reasons.is_empty()) {
        let mut seen = HashSet::from([c.page.id]);
        let mut at = c.page.id;
        while let Some(parent) = parent_of.get(&at).copied() {
            if !seen.insert(parent) {
                break;
            }
            *kept_under.entry(parent).or_default() += 1;
            at = parent;
        }
    }

    let mut kept = Vec::new();
    let mut deletable_ids = Vec::new();
    for c in checked {
        let Some(state) = c.state else {
            continue;
        };
        let mut reasons = c.reasons;
        if reasons.is_empty() {
            if let Some(count) = kept_under.get(&c.page.id).copied() {
                reasons.push(KeptReason::KeptPagesUnder { count });
            }
        }
        if reasons.is_empty() {
            deletable_ids.push(c.page.id);
        } else {
            reasons.sort();
            kept.push(KeptPage {
                id: c.page.id,
                title: state.title,
                reasons,
            });
        }
    }

    let collections = collections_outcome(conn, record, &deletable_ids, lock)?;
    Ok(RemovalPlan {
        created,
        deletable: deletable_ids.len(),
        gone,
        kept,
        collections,
        deletable_ids,
    })
}

/// The collections the import created that still exist: deleted when
/// every page in them outside Trash would move to Trash, else kept with
/// how many pages stay.
fn collections_outcome(
    conn: &mut DbConnection,
    record: &CommitRecord,
    deletable: &[i32],
    lock: bool,
) -> Result<CollectionsOutcome, DieselError> {
    let states = repo::collection_states(conn, &record.collections_created, lock)?;
    let ids: Vec<i32> = states.iter().map(|c| c.id).collect();
    let deletable: HashSet<i32> = deletable.iter().copied().collect();
    let mut staying: HashMap<i32, usize> = HashMap::new();
    for (collection, page) in repo::live_collection_pages(conn, &ids)? {
        if !deletable.contains(&page) {
            *staying.entry(collection).or_default() += 1;
        }
    }
    let mut outcome = CollectionsOutcome::default();
    for c in states {
        let pages = staying.get(&c.id).copied().unwrap_or(0);
        let entry = CollectionOutcome {
            id: c.id,
            name: c.name,
            pages,
        };
        if pages == 0 {
            outcome.deleted.push(entry);
        } else {
            outcome.kept.push(entry);
        }
    }
    Ok(outcome)
}

/// Move the pages `plan` found untouched to Trash, each with its sync
/// event, and delete the collections it found empty: their pages go to
/// Trash with them, as deleting a collection does. Run inside the
/// transaction that made `plan` with `lock`. Returns the result and the
/// ids now in Trash, for the search index once the transaction commits.
pub fn remove(
    conn: &mut DbConnection,
    plan: RemovalPlan,
) -> Result<(RemoveResult, Vec<i32>), DieselError> {
    let in_deleted: HashSet<i32> = {
        let ids: Vec<i32> = plan.collections.deleted.iter().map(|c| c.id).collect();
        repo::live_collection_pages(conn, &ids)?
            .into_iter()
            .map(|(_, page)| page)
            .collect()
    };
    let now = Utc::now().naive_utc();
    let to_trash = DocumentationPageUpdate {
        status: Some(DocumentationStatus::Deleted),
        archived_at: Some(None),
        updated_at: Some(now),
        deleted_at: Some(Some(now)),
        ..Default::default()
    };
    let mut trashed = Vec::with_capacity(plan.deletable_ids.len());
    for id in &plan.deletable_ids {
        if !in_deleted.contains(id) {
            repository::update_documentation_page(conn, *id, &to_trash)?;
        }
        trashed.push(*id);
    }
    for c in &plan.collections.deleted {
        collections::soft_delete_pages_in_collection(conn, c.id)?;
        collections::delete_collection(conn, c.id)?;
    }
    Ok((
        RemoveResult {
            kind: JOB_TYPE.to_string(),
            created: plan.created,
            deleted: trashed.len(),
            gone: plan.gone,
            deleted_ids: trashed.clone(),
            kept: plan.kept,
            collections: plan.collections,
        },
        trashed,
    ))
}

/// What Publish all would do now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublishPlan {
    /// Untouched pages still in draft: published.
    pub publishable: usize,
    /// Pages changed since the import: left as they are.
    pub left: usize,
    #[serde(skip)]
    pub publishable_ids: Vec<i32>,
}

/// The import's pages Publish all would publish: in draft and untouched in
/// content and place. Pages changed since are counted as left; pages
/// published as imported, or in Trash, are neither. With `lock`, the pages
/// are locked for the rest of the transaction.
pub fn publish_plan(
    conn: &mut DbConnection,
    record: &CommitRecord,
    lock: bool,
) -> Result<PublishPlan, DieselError> {
    let checked = check(conn, record, lock)?;
    let mut publishable_ids = Vec::new();
    let mut left = 0;
    for c in &checked {
        let Some(state) = &c.state else {
            continue;
        };
        let changed = c
            .reasons
            .iter()
            .any(|r| matches!(r, KeptReason::Edited { .. } | KeptReason::Moved));
        if changed {
            left += 1;
        } else if status_is(state, "draft") {
            publishable_ids.push(c.page.id);
        }
    }
    Ok(PublishPlan {
        publishable: publishable_ids.len(),
        left,
        publishable_ids,
    })
}

/// Publish the pages `plan` found, each with its sync event, and record
/// their new `updated_at` in `record`, so the rule still sees them
/// untouched. Run inside the transaction that made `plan` with `lock`.
/// Returns the published pages, for the search index.
pub fn publish(
    conn: &mut DbConnection,
    record: &mut CommitRecord,
    plan: &PublishPlan,
) -> Result<Vec<DocumentationPage>, DieselError> {
    let update = DocumentationPageUpdate {
        status: Some(DocumentationStatus::Published),
        ..Default::default()
    };
    let mut published = Vec::with_capacity(plan.publishable_ids.len());
    let mut stamps: HashMap<i32, NaiveDateTime> = HashMap::new();
    for id in &plan.publishable_ids {
        let page = repository::update_documentation_page(conn, *id, &update)?;
        stamps.insert(page.id, page.updated_at);
        published.push(page);
    }
    for page in &mut record.pages {
        if let Some(at) = stamps.get(&page.id) {
            page.updated_at = *at;
        }
    }
    Ok(published)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorded_page(id: i32) -> RecordedPage {
        RecordedPage {
            id,
            uuid: Uuid::now_v7(),
            index: id as usize,
            source: format!("{id}.md"),
            collection_id: Some(7),
            updated_at: NaiveDateTime::parse_from_str("2026-10-19 01:02:03", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            hash: "ab".repeat(32),
        }
    }

    fn state_of(page: &RecordedPage) -> repo::PageState {
        repo::PageState {
            id: page.id,
            title: "Page".into(),
            status: "draft".into(),
            parent_id: None,
            updated_at: page.updated_at,
            hash: Some(page.hash.to_uppercase()),
            created_by: Uuid::nil(),
            last_edited_by: Uuid::max(),
            collection_id: page.collection_id,
        }
    }

    #[test]
    fn a_page_as_imported_has_no_reasons() {
        let page = recorded_page(1);
        assert!(own_reasons(&page, &state_of(&page), None).is_empty());
        let first = repo::NewestRevision {
            page_id: 1,
            revision_number: 1,
            created_at: Utc::now(),
            created_by: Uuid::nil(),
        };
        assert!(own_reasons(&page, &state_of(&page), Some(&first)).is_empty());
    }

    #[test]
    fn a_detail_change_names_who_made_it() {
        let page = recorded_page(1);
        let mut state = state_of(&page);
        state.updated_at += chrono::Duration::seconds(5);
        let reasons = own_reasons(&page, &state, None);
        assert!(matches!(
            reasons.as_slice(),
            [KeptReason::Edited { by: Some(by), .. }] if *by == Uuid::max()
        ));
    }

    #[test]
    fn a_later_revision_names_its_author_and_time() {
        let page = recorded_page(1);
        let mut state = state_of(&page);
        state.hash = Some("cd".repeat(32));
        let author = Uuid::now_v7();
        let at = Utc::now();
        let rev = repo::NewestRevision {
            page_id: 1,
            revision_number: 2,
            created_at: at,
            created_by: author,
        };
        let reasons = own_reasons(&page, &state, Some(&rev));
        assert_eq!(
            reasons,
            vec![KeptReason::Edited {
                at,
                by: Some(author),
                by_name: None
            }]
        );
    }

    #[test]
    fn a_content_change_without_a_revision_names_nobody() {
        let page = recorded_page(1);
        let mut state = state_of(&page);
        state.hash = None;
        assert!(matches!(
            own_reasons(&page, &state, None).as_slice(),
            [KeptReason::Edited { by: None, .. }]
        ));
    }

    #[test]
    fn a_move_is_its_own_reason() {
        let page = recorded_page(1);
        let mut state = state_of(&page);
        state.collection_id = Some(8);
        assert_eq!(own_reasons(&page, &state, None), vec![KeptReason::Moved]);
    }

    #[test]
    fn reasons_serialize_with_their_code_most_telling_first() {
        let mut reasons = [
            KeptReason::KeptPagesUnder { count: 1 },
            KeptReason::Moved,
            KeptReason::PagesAdded { count: 2 },
            KeptReason::Edited {
                at: Utc::now(),
                by: None,
                by_name: None,
            },
        ];
        reasons.sort();
        assert!(matches!(reasons[0], KeptReason::Edited { .. }));
        assert_eq!(
            serde_json::to_value(&reasons[1]).unwrap(),
            serde_json::json!({ "code": "pages_added", "count": 2 })
        );
        assert_eq!(
            serde_json::to_value(&reasons[2]).unwrap(),
            serde_json::json!({ "code": "moved" })
        );
        assert_eq!(
            serde_json::to_value(&reasons[3]).unwrap(),
            serde_json::json!({ "code": "kept_pages_under", "count": 1 })
        );
    }
}
