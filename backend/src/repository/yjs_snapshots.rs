//! Crash-recovery checkpoints for collaborative documents.
//!
//! The collaboration checkpoint loop writes a cheap binary snapshot here
//! between the heavier `article_contents` saves, so a hard crash (SIGKILL
//! / OOM / panic) loses seconds of edits rather than the whole save
//! interval. On document open, the latest checkpoint is merged on top of
//! whatever loaded from Redis / `article_contents`; Yjs merges are
//! conflict-free + idempotent, so it can only add missing ops.
//!
//! All access is workspace-scoped: the table has FORCE RLS keyed on
//! `app.workspace_id`, so every call runs under the per-doc session actor
//! that sets it.

use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::NewYjsSnapshot;
use crate::schema::yjs_snapshots;

/// How many checkpoints to retain per document. Recovery only ever reads
/// the newest; a tiny tail is kept as cheap insurance against a torn
/// final write. Pruned on every insert so the table can't grow unbounded.
const KEEP_PER_DOCUMENT: i64 = 3;

// sync-audit-only: collaborative-editing crash-recovery checkpoints; workspace-scoped via RLS, never user-observable
/// Append a checkpoint for `document_id`, then trim that document's
/// checkpoints to the newest [`KEEP_PER_DOCUMENT`]. Insert + prune share
/// one transaction (one workspace elevation, RLS-enforced on both).
pub fn insert_and_prune(
    conn: &mut DbConnection,
    workspace_id: i32,
    document_id: &str,
    snapshot: &[u8],
    state_vector: &[u8],
) -> QueryResult<()> {
    conn.transaction(|conn| {
        diesel::insert_into(yjs_snapshots::table)
            .values(&NewYjsSnapshot {
                workspace_id,
                document_id,
                snapshot,
                state_vector,
            })
            .execute(conn)?;

        // Ids of the newest N to keep for this doc; delete the rest. Tie
        // on created_at is broken by the monotonic id so the keep set is
        // deterministic when several checkpoints share a timestamp.
        let keep: Vec<i64> = yjs_snapshots::table
            .filter(yjs_snapshots::workspace_id.eq(workspace_id))
            .filter(yjs_snapshots::document_id.eq(document_id))
            .order((yjs_snapshots::created_at.desc(), yjs_snapshots::id.desc()))
            .limit(KEEP_PER_DOCUMENT)
            .select(yjs_snapshots::id)
            .load(conn)?;

        diesel::delete(
            yjs_snapshots::table
                .filter(yjs_snapshots::workspace_id.eq(workspace_id))
                .filter(yjs_snapshots::document_id.eq(document_id))
                .filter(yjs_snapshots::id.ne_all(keep)),
        )
        .execute(conn)?;

        Ok(())
    })
}

/// The most recent checkpoint blob for `document_id`, if any. Returns the
/// full Yjs v1 update to apply on resume.
pub fn latest_for_document(
    conn: &mut DbConnection,
    workspace_id: i32,
    document_id: &str,
) -> QueryResult<Option<Vec<u8>>> {
    yjs_snapshots::table
        .filter(yjs_snapshots::workspace_id.eq(workspace_id))
        .filter(yjs_snapshots::document_id.eq(document_id))
        .order((yjs_snapshots::created_at.desc(), yjs_snapshots::id.desc()))
        .select(yjs_snapshots::snapshot)
        .first::<Vec<u8>>(conn)
        .optional()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;

    // setup_test_connection pins app.workspace_id = 1 and runs as the
    // RLS-enforced app role, so these exercise the real tenant policy.
    const WS: i32 = 1;

    #[test]
    fn insert_and_prune_keeps_only_latest_n() {
        let mut conn = setup_test_connection();
        let doc = "ws-test_ticket-9001";

        // Write more checkpoints than we retain; each carries a
        // distinguishable payload so we can assert which survive.
        let total = KEEP_PER_DOCUMENT + 2;
        for i in 0..total {
            let snapshot = vec![i as u8; 4];
            let state_vector = vec![i as u8; 2];
            insert_and_prune(&mut conn, WS, doc, &snapshot, &state_vector).unwrap();
        }

        let remaining: i64 = yjs_snapshots::table
            .filter(yjs_snapshots::workspace_id.eq(WS))
            .filter(yjs_snapshots::document_id.eq(doc))
            .count()
            .get_result(&mut conn)
            .unwrap();
        assert_eq!(
            remaining, KEEP_PER_DOCUMENT,
            "prune must cap per-document rows"
        );

        // The most recent insert wins.
        let latest = latest_for_document(&mut conn, WS, doc).unwrap();
        assert_eq!(latest, Some(vec![(total - 1) as u8; 4]));
    }

    #[test]
    fn latest_for_document_is_none_when_absent() {
        let mut conn = setup_test_connection();
        let latest = latest_for_document(&mut conn, WS, "ws-test_ticket-absent").unwrap();
        assert_eq!(latest, None);
    }
}
