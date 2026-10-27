//! Rebuilding one workspace's search documents.
//!
//! A full rebuild (`SearchService::rebuild_index`) replaces every workspace's
//! documents, which a hosted machine must not do for one tenant's import. This
//! rebuilds one workspace and leaves the others alone.
//!
//! **Upsert, then prune.** The pass first notes which documents the index
//! holds for the workspace, then indexes the workspace's rows page by page
//! (each upsert replaces the document with the same id), and only at the end
//! deletes the noted documents whose rows are gone. It never deletes by the
//! workspace term: a person's document carries every workspace they belong to,
//! so that delete would drop the workspace's people from every other workspace
//! too. The writer lock is taken per page, not for the whole pass, so the
//! incremental index tasks keep running. When one of them commits mid-pass,
//! searchers see a mix of old and refreshed documents, never a missing one;
//! documents for deleted rows stay findable until the final prune commits. A
//! row created during the pass is not in the snapshot, so it is never pruned.
//!
//! **One pass per workspace at a time.** A request that arrives before a
//! pass starts reading joins it. A request that arrives while a pass is
//! reading queues one more pass, since the running one may have read a table
//! before that request's rows were committed. Any number of such requests
//! queue one pass between them.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Mutex;

use tantivy::collector::DocSetCollector;
use tantivy::query::TermQuery;
use tantivy::schema::{IndexRecordOption, Value};
use tantivy::{TantivyDocument, Term};
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::repository::search_reindex as repo;
use crate::sync::session::{background_run, run_in_workspace};

use super::indexer::{self, IndexStats};
use super::types::{EntityType, IndexDocument};
use super::SearchService;

type Error = Box<dyn std::error::Error + Send + Sync>;

/// Rows read and indexed per page.
const PAGE: i64 = 500;

const REF: &str = "background:search_reindex_workspace";

/// What one workspace reindex did.
#[derive(Debug, Default)]
pub struct WorkspaceIndexStats {
    /// Documents written, by kind.
    pub indexed: IndexStats,
    /// Documents removed because their rows are gone.
    pub removed: usize,
}

/// How a request for a workspace reindex was taken.
#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    /// No pass was pending: the caller runs one.
    Start,
    /// A pass is already pending or running; it (or the one queued after it)
    /// covers this request.
    Joined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// Admitted, not yet reading.
    Waiting,
    /// Reading; `again` when a request arrived since it started.
    Running { again: bool },
}

/// The workspaces with a reindex pending or running.
#[derive(Default)]
pub struct ReindexSlots(Mutex<HashMap<i32, Slot>>);

impl ReindexSlots {
    fn slots(&self) -> std::sync::MutexGuard<'_, HashMap<i32, Slot>> {
        // The map holds plain flags, so a poisoned lock's data is still sound.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Take a request for `workspace_id`.
    pub fn admit(&self, workspace_id: i32) -> Admission {
        let mut slots = self.slots();
        match slots.get_mut(&workspace_id) {
            None => {
                slots.insert(workspace_id, Slot::Waiting);
                Admission::Start
            }
            Some(Slot::Waiting) => Admission::Joined,
            Some(Slot::Running { again }) => {
                *again = true;
                Admission::Joined
            }
        }
    }

    fn begin_pass(&self, workspace_id: i32) {
        self.slots()
            .insert(workspace_id, Slot::Running { again: false });
    }

    /// End a pass. True when another was requested meanwhile; the slot stays
    /// held for it. Otherwise the slot is released.
    fn end_pass(&self, workspace_id: i32) -> bool {
        let mut slots = self.slots();
        if slots.get(&workspace_id) == Some(&Slot::Running { again: true }) {
            slots.insert(workspace_id, Slot::Waiting);
            true
        } else {
            slots.remove(&workspace_id);
            false
        }
    }

    fn release(&self, workspace_id: i32) {
        self.slots().remove(&workspace_id);
    }

    /// Whether a reindex of `workspace_id` is pending or running.
    pub fn is_held(&self, workspace_id: i32) -> bool {
        self.slots().contains_key(&workspace_id)
    }
}

/// Releases a slot when a run ends by error or panic.
struct Held<'a> {
    slots: &'a ReindexSlots,
    workspace_id: i32,
    armed: bool,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.slots.release(self.workspace_id);
        }
    }
}

/// What the index held for a workspace when a pass began.
struct Snapshot {
    /// Ids of the workspace's own documents (everything but people).
    owned: HashSet<String>,
    /// People whose document carries the workspace.
    people: Vec<Uuid>,
}

impl SearchService {
    /// Run the reindex admitted for `workspace_id` (`Admission::Start`), and
    /// any pass queued while it runs, then commit. Blocking: call it off the
    /// async runtime (`indexing_tasks::spawn_reindex_workspace` does).
    pub fn run_admitted_workspace_reindex(
        &self,
        workspace_id: i32,
    ) -> Result<WorkspaceIndexStats, Error> {
        let mut held = Held {
            slots: &self.reindex_slots,
            workspace_id,
            armed: true,
        };
        loop {
            self.reindex_slots.begin_pass(workspace_id);
            let stats = self.reindex_workspace_pass(workspace_id)?;
            if !self.reindex_slots.end_pass(workspace_id) {
                held.armed = false;
                return Ok(stats);
            }
        }
    }

    /// Take a request to reindex `workspace_id`. On `Admission::Start` the
    /// caller must run [`Self::run_admitted_workspace_reindex`].
    pub fn admit_workspace_reindex(&self, workspace_id: i32) -> Admission {
        self.reindex_slots.admit(workspace_id)
    }

    /// Whether a reindex of `workspace_id` is pending or running.
    pub fn is_reindexing_workspace(&self, workspace_id: i32) -> bool {
        self.reindex_slots.is_held(workspace_id)
    }

    /// Reindex `workspace_id` now, on this thread. `None` when a reindex of
    /// the workspace is already pending or running, which covers this request.
    pub fn reindex_workspace(
        &self,
        workspace_id: i32,
    ) -> Result<Option<WorkspaceIndexStats>, Error> {
        match self.admit_workspace_reindex(workspace_id) {
            Admission::Start => self.run_admitted_workspace_reindex(workspace_id).map(Some),
            Admission::Joined => Ok(None),
        }
    }

    fn reindex_workspace_pass(&self, workspace_id: i32) -> Result<WorkspaceIndexStats, Error> {
        info!(workspace_id, "Reindexing workspace");
        let snapshot = self.workspace_snapshot(workspace_id)?;
        let mut seen: HashSet<String> = HashSet::new();
        let mut stats = WorkspaceIndexStats::default();

        // The ticket documents carry their state's category, read from memory.
        run_in_workspace(&self.pool, REF, workspace_id, |conn| {
            crate::repository::workflow_states::remember_visible_categories(conn)
        })?;

        stats.indexed.tickets = self.index_pages(
            workspace_id,
            &mut seen,
            |conn, after| {
                let tickets = repo::tickets_after(conn, workspace_id, after, PAGE)?;
                let ids: Vec<i32> = tickets.iter().map(|t| t.id).collect();
                let mut articles: HashMap<i32, _> =
                    repo::article_contents_for(conn, workspace_id, &ids)?
                        .into_iter()
                        .filter_map(|a| a.ticket_id.map(|id| (id, a)))
                        .collect();
                Ok(tickets
                    .into_iter()
                    .map(|t| {
                        let article = articles.remove(&t.id);
                        (t, article)
                    })
                    .collect())
            },
            |(t, _)| t.id,
            |(t, article)| indexer::index_document_from_ticket(t, article.as_ref()),
        )?;

        stats.indexed.comments = self.index_pages(
            workspace_id,
            &mut seen,
            |conn, after| {
                let comments = repo::comments_after(conn, workspace_id, after, PAGE)?;
                let ticket_ids: Vec<i32> = comments.iter().map(|c| c.ticket_id).collect();
                let titles = repo::ticket_titles(conn, workspace_id, &ticket_ids)?;
                Ok(comments
                    .into_iter()
                    .map(|c| {
                        let title = titles
                            .get(&c.ticket_id)
                            .cloned()
                            .unwrap_or_else(|| "Unknown Ticket".to_string());
                        (c, title)
                    })
                    .collect())
            },
            |(c, _)| c.id,
            |(c, title)| indexer::index_document_from_comment(c, title),
        )?;

        stats.indexed.attachments = self.index_pages(
            workspace_id,
            &mut seen,
            |conn, after| repo::transcribed_attachments_after(conn, workspace_id, after, PAGE),
            |(a, _, _)| a.id,
            |(a, ticket_id, title)| indexer::index_document_from_attachment(a, *ticket_id, title),
        )?;

        stats.indexed.documentation = self.index_pages(
            workspace_id,
            &mut seen,
            |conn, after| repo::documentation_pages_after(conn, workspace_id, after, PAGE),
            |p| p.id,
            indexer::index_document_from_documentation,
        )?;

        stats.indexed.devices = self.index_pages(
            workspace_id,
            &mut seen,
            |conn, after| repo::assets_after(conn, workspace_id, after, PAGE),
            |a| a.id,
            indexer::index_document_from_device,
        )?;

        stats.indexed.projects = self.index_pages(
            workspace_id,
            &mut seen,
            |conn, after| repo::projects_after(conn, workspace_id, after, PAGE),
            |p| p.id,
            indexer::index_document_from_project,
        )?;

        stats.indexed.users = self.reindex_people(workspace_id, snapshot.people)?;

        // Prune the documents whose rows are gone.
        let gone: Vec<&String> = snapshot.owned.difference(&seen).collect();
        if !gone.is_empty() {
            let writer = self.writer_lock()?;
            for id in &gone {
                writer.delete_term(Term::from_field_text(self.schema.id, id));
            }
        }
        stats.removed = gone.len();

        self.commit()?;
        info!(
            workspace_id,
            total = stats.indexed.total(),
            count = stats.removed,
            "Workspace reindexed"
        );
        Ok(stats)
    }

    /// The documents the index holds for `workspace_id`, as of now.
    fn workspace_snapshot(&self, workspace_id: i32) -> Result<Snapshot, Error> {
        self.reader.reload()?;
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_i64(self.schema.workspace_id, workspace_id as i64),
            IndexRecordOption::Basic,
        );
        let user_prefix = format!("{}-", EntityType::User.as_str());
        let mut snapshot = Snapshot {
            owned: HashSet::new(),
            people: Vec::new(),
        };
        for address in searcher.search(&query, &DocSetCollector)? {
            let doc: TantivyDocument = searcher.doc(address)?;
            let Some(id) = doc
                .get_first(self.schema.id)
                .and_then(|v| v.as_str().map(str::to_owned))
            else {
                continue;
            };
            match id.strip_prefix(&user_prefix) {
                Some(uuid) => {
                    if let Ok(uuid) = Uuid::parse_str(uuid) {
                        snapshot.people.push(uuid);
                    }
                }
                None => {
                    snapshot.owned.insert(id);
                }
            }
        }
        Ok(snapshot)
    }

    /// Index every row a pager returns, a page at a time: each page is read in
    /// its own transaction pinned to the workspace, then written under one
    /// short hold of the writer lock.
    fn index_pages<T>(
        &self,
        workspace_id: i32,
        seen: &mut HashSet<String>,
        mut load: impl FnMut(&mut DbConnection, i32) -> diesel::QueryResult<Vec<T>>,
        key: impl Fn(&T) -> i32,
        to_doc: impl Fn(&T) -> IndexDocument,
    ) -> Result<usize, Error> {
        let mut after = 0;
        let mut indexed = 0;
        loop {
            let page = run_in_workspace(&self.pool, REF, workspace_id, |conn| load(conn, after))?;
            let Some(last) = page.last() else {
                break;
            };
            after = key(last);
            let docs: Vec<IndexDocument> = page.iter().map(&to_doc).collect();
            indexed += self.write_docs(&docs, seen)?;
            if (page.len() as i64) < PAGE {
                break;
            }
        }
        Ok(indexed)
    }

    /// Upsert `docs`, noting each id in `seen`. Returns how many were written.
    fn write_docs(
        &self,
        docs: &[IndexDocument],
        seen: &mut HashSet<String>,
    ) -> Result<usize, Error> {
        let writer = self.writer_lock()?;
        let mut written = 0;
        for doc in docs {
            seen.insert(doc.id.clone());
            match indexer::add_document_to_index(&writer, &self.schema, doc) {
                Ok(()) => written += 1,
                Err(e) => {
                    warn!(error = ?e, "Failed to index a document during a workspace reindex")
                }
            }
        }
        Ok(written)
    }

    /// Rewrite the document of every active member and of everyone whose
    /// document carried the workspace, from their current memberships. A
    /// person who no longer exists (or is deleted) loses their document, as
    /// the user-deleted hook removes it.
    fn reindex_people(&self, workspace_id: i32, tagged: Vec<Uuid>) -> Result<usize, Error> {
        let members = run_in_workspace(&self.pool, REF, workspace_id, |conn| {
            repo::active_member_uuids(conn, workspace_id)
        })?;
        let people: Vec<Uuid> = members
            .into_iter()
            .chain(tagged)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        let mut indexed = 0;
        for chunk in people.chunks(PAGE as usize) {
            // cross-tenant: a person's document carries every workspace they belong to.
            let loaded = background_run(
                &self.pool,
                "background:search_reindex_workspace_people",
                |conn| repo::users_for_index(conn, chunk),
            )?;
            let found: HashSet<Uuid> = loaded.iter().map(|(u, _, _)| u.uuid).collect();
            let docs: Vec<IndexDocument> = loaded
                .iter()
                .map(|(user, email, workspace_ids)| {
                    indexer::index_document_from_user(user, email.as_deref(), workspace_ids)
                })
                .collect();
            let mut ignored = HashSet::new();
            indexed += self.write_docs(&docs, &mut ignored)?;

            let gone: Vec<&Uuid> = chunk.iter().filter(|u| !found.contains(u)).collect();
            if !gone.is_empty() {
                let writer = self.writer_lock()?;
                for uuid in gone {
                    indexer::delete_document_from_index(
                        &writer,
                        &self.schema,
                        EntityType::User,
                        &uuid.to_string(),
                    )?;
                }
            }
        }
        Ok(indexed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_before_the_pass_reads_joins_it() {
        let slots = ReindexSlots::default();
        assert_eq!(slots.admit(7), Admission::Start);
        assert_eq!(slots.admit(7), Admission::Joined);
        slots.begin_pass(7);
        assert!(!slots.end_pass(7), "no second pass");
        assert!(!slots.is_held(7));
    }

    #[test]
    fn a_request_while_the_pass_reads_queues_one_more() {
        let slots = ReindexSlots::default();
        assert_eq!(slots.admit(7), Admission::Start);
        slots.begin_pass(7);
        assert_eq!(slots.admit(7), Admission::Joined);
        assert_eq!(slots.admit(7), Admission::Joined);
        assert!(slots.end_pass(7), "one more pass");
        assert!(slots.is_held(7));
        slots.begin_pass(7);
        assert!(!slots.end_pass(7));
        assert!(!slots.is_held(7));
    }

    #[test]
    fn workspaces_do_not_share_a_slot() {
        let slots = ReindexSlots::default();
        assert_eq!(slots.admit(1), Admission::Start);
        assert_eq!(slots.admit(2), Admission::Start);
    }

    #[test]
    fn a_failed_run_releases_its_slot() {
        let slots = ReindexSlots::default();
        assert_eq!(slots.admit(3), Admission::Start);
        drop(Held {
            slots: &slots,
            workspace_id: 3,
            armed: true,
        });
        assert_eq!(slots.admit(3), Admission::Start);
    }
}
