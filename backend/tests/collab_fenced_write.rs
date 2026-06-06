//! Integration tests for the fencing guard on collaborative-editing
//! snapshot writes (Phase 2 affinity). See
//! `docs/realtime-collab-affinity-design.md`.
//!
//! This is the DB half of the split-brain protection: a snapshot write
//! carries the owning machine's monotonic fence token, and a write from a
//! stale owner (lower token) must be rejected rather than clobber the new
//! owner's state. We exercise it through the collection write path
//! (`update_collection_description_yjs`) because it has the lightest
//! fixture (no ticket/user FKs) and returns the affected row count, which
//! makes the applied-vs-rejected assertion direct. The article and
//! documentation-page write paths use the identical guard shape.
//!
//! Each test runs against its own template-cloned sandbox DB
//! (`common::TestDb`), with `app.workspace_id = 1` pinned by the pool's
//! GUC customizer so audited writes satisfy their workspace constraint.

#![allow(clippy::expect_used)] // tests fail loudly on purpose

use backend::models::NewDocumentationCollection;
use backend::repository::documentation_collections::{
    create_collection, get_collection, update_collection_description_yjs,
};

mod common;

fn make_collection(conn: &mut backend::db::DbConnection) -> i32 {
    create_collection(
        conn,
        NewDocumentationCollection {
            uuid: uuid::Uuid::new_v4(),
            name: "Fence Test".into(),
            slug: "fence-test".into(),
            description: None,
            icon: None,
            color: None,
            is_system: false,
            created_by: None,
        },
    )
    .expect("create collection fixture")
    .id
}

/// A stale fence (lower than what's stored) is rejected and leaves the
/// content untouched; an equal-or-higher fence is applied. This is the
/// guarantee that protects the new owner's snapshot during an ownership
/// handoff overlap.
#[actix_web::test]
async fn stale_fence_rejected_newer_applied() {
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let id = make_collection(&mut conn);

    // fence_token starts NULL, so the first fenced write (token 5) lands.
    let n = update_collection_description_yjs(&mut conn, id, b"doc-a".to_vec(), Some(5))
        .expect("first fenced write");
    assert_eq!(
        n, 1,
        "first fenced write should apply (stored fence was NULL)"
    );
    let c = get_collection(&mut conn, id).expect("reload");
    assert_eq!(c.fence_token, Some(5));
    assert_eq!(c.description_yjs.as_deref(), Some(&b"doc-a"[..]));

    // Stale: token 3 < stored 5 → rejected (0 rows), content unchanged.
    let n = update_collection_description_yjs(&mut conn, id, b"doc-b".to_vec(), Some(3))
        .expect("stale write call");
    assert_eq!(n, 0, "a stale fence must be rejected");
    let c = get_collection(&mut conn, id).expect("reload");
    assert_eq!(
        c.fence_token,
        Some(5),
        "fence must be unchanged after a stale write"
    );
    assert_eq!(
        c.description_yjs.as_deref(),
        Some(&b"doc-a"[..]),
        "content must be unchanged after a stale write"
    );

    // Newer: token 6 >= 5 → applies.
    let n = update_collection_description_yjs(&mut conn, id, b"doc-c".to_vec(), Some(6))
        .expect("newer write");
    assert_eq!(n, 1, "a newer fence should apply");
    let c = get_collection(&mut conn, id).expect("reload");
    assert_eq!(c.fence_token, Some(6));
    assert_eq!(c.description_yjs.as_deref(), Some(&b"doc-c"[..]));

    // Equal fence (e.g. the owner re-saving at its current token) still
    // applies, since the guard is `fence_token <= f`.
    let n = update_collection_description_yjs(&mut conn, id, b"doc-d".to_vec(), Some(6))
        .expect("equal-fence write");
    assert_eq!(n, 1, "an equal fence should still apply (guard is <=)");
}

/// The `None` fence (single-instance / Redis-down path) writes
/// unconditionally and leaves the stored fence token untouched, matching
/// pre-affinity behaviour.
#[actix_web::test]
async fn unfenced_write_is_unconditional() {
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let id = make_collection(&mut conn);

    // Stamp a high fence via a fenced write.
    update_collection_description_yjs(&mut conn, id, b"doc-a".to_vec(), Some(100))
        .expect("seed fence");

    // None writes regardless of the stored fence (would be rejected if it
    // were fenced at token < 100).
    let n = update_collection_description_yjs(&mut conn, id, b"doc-z".to_vec(), None)
        .expect("unfenced write");
    assert_eq!(n, 1, "an unfenced write always applies");

    let c = get_collection(&mut conn, id).expect("reload");
    assert_eq!(c.description_yjs.as_deref(), Some(&b"doc-z"[..]));
    assert_eq!(
        c.fence_token,
        Some(100),
        "the unfenced path must not touch the stored fence token"
    );
}
