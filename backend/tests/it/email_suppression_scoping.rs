//! The email suppression list is per workspace.
//!
//! It used to be keyed on the address alone with no `workspace_id`, so the RLS
//! GUC that `TenantConn` primes was a no-op and the three admin endpoints had
//! no tenant predicate: any workspace admin could read, extend and delete every
//! other tenant's list. Reading it is the sharper half, since the list is a
//! roster of addresses a tenant has corresponded with.
//!
//! **What these tests cover.** The test role is a superuser with BYPASSRLS, so
//! row-level security does not apply here. That is deliberate about what is
//! being asserted: every query carries an explicit `workspace_id` filter, and
//! it is the filter, not RLS, that makes these queries correct. RLS is the
//! backstop for a caller that forgets to pin, and the migration's
//! ENABLE/FORCE/policy is covered by `tenant_table_rls_lint`.

#![allow(clippy::expect_used)]

use backend::db::DbConnection;
use backend::models::{email_suppression_reason, NewEmailSuppression};
use backend::repository::email_suppressions as repo;

fn suppress(conn: &mut DbConnection, workspace_id: i32, email: &str) {
    repo::upsert(
        conn,
        NewEmailSuppression {
            email: email.to_string(),
            reason: email_suppression_reason::HARD_BOUNCE.to_string(),
            bounce_diagnostic: Some("550 user unknown".into()),
            workspace_id,
        },
    )
    .expect("upsert suppression");
}

#[test]
fn one_workspace_suppressing_an_address_does_not_silence_another() {
    crate::common::ensure_test_keyring();
    let test_db = crate::common::TestDb::new();
    let pool = test_db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");

    let ours = crate::common::mint_workspace(&mut conn, "acme-supp-ours", "Ours");
    let theirs = crate::common::mint_workspace(&mut conn, "acme-supp-theirs", "Theirs");

    suppress(&mut conn, ours, "shared.contact@example.org");

    assert!(
        repo::is_suppressed(&mut conn, ours, "shared.contact@example.org").expect("query"),
        "the workspace that recorded the bounce stops writing to them"
    );
    assert!(
        !repo::is_suppressed(&mut conn, theirs, "shared.contact@example.org").expect("query"),
        "a peer tenant's bounce must not silence our mail to our own contact"
    );
}

#[test]
fn the_admin_list_shows_only_this_workspaces_addresses() {
    crate::common::ensure_test_keyring();
    let test_db = crate::common::TestDb::new();
    let pool = test_db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");

    let ours = crate::common::mint_workspace(&mut conn, "acme-supp-list", "Ours List");
    let theirs = crate::common::mint_workspace(&mut conn, "acme-supp-list-x", "Theirs List");

    suppress(&mut conn, ours, "ours@example.org");
    suppress(&mut conn, theirs, "theirs@example.org");

    let rows = repo::list(&mut conn, ours, 50, None).expect("list");
    let emails: Vec<&str> = rows.iter().map(|r| r.email.as_str()).collect();
    assert!(emails.contains(&"ours@example.org"));
    assert!(
        !emails.contains(&"theirs@example.org"),
        "the list is a roster of addresses a tenant corresponds with: {emails:?}"
    );
    assert_eq!(
        repo::count(&mut conn, ours).expect("count"),
        1,
        "the stats card counts this workspace only"
    );
}

/// The primary key moved from `(email)` to `(workspace_id, email)`, so the same
/// person can be a live contact of one tenant and a hard bounce for another.
#[test]
fn two_workspaces_can_suppress_the_same_address_independently() {
    crate::common::ensure_test_keyring();
    let test_db = crate::common::TestDb::new();
    let pool = test_db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");

    let a = crate::common::mint_workspace(&mut conn, "acme-supp-a", "A");
    let b = crate::common::mint_workspace(&mut conn, "acme-supp-b", "B");

    suppress(&mut conn, a, "both@example.org");
    suppress(&mut conn, b, "both@example.org");

    assert!(repo::is_suppressed(&mut conn, a, "both@example.org").expect("query"));
    assert!(repo::is_suppressed(&mut conn, b, "both@example.org").expect("query"));

    // Removing one leaves the other, which is the whole point: a peer must not
    // be able to resume mail to someone who asked us to stop.
    assert_eq!(
        repo::remove(&mut conn, a, "both@example.org").expect("remove"),
        1
    );
    assert!(!repo::is_suppressed(&mut conn, a, "both@example.org").expect("query"));
    assert!(
        repo::is_suppressed(&mut conn, b, "both@example.org").expect("query"),
        "removing one workspace's entry must not clear another's"
    );
}

/// Deleting an address that is only on a peer's list reports "not on the list",
/// the same answer as an address nobody has suppressed. No cross-tenant oracle.
#[test]
fn removing_a_peers_entry_reports_nothing_removed() {
    crate::common::ensure_test_keyring();
    let test_db = crate::common::TestDb::new();
    let pool = test_db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");

    let ours = crate::common::mint_workspace(&mut conn, "acme-supp-del", "Ours Del");
    let theirs = crate::common::mint_workspace(&mut conn, "acme-supp-del-x", "Theirs Del");
    suppress(&mut conn, theirs, "theirs.only@example.org");

    assert_eq!(
        repo::remove(&mut conn, ours, "theirs.only@example.org").expect("remove"),
        0,
        "a peer's entry is not ours to delete"
    );
    assert!(
        repo::is_suppressed(&mut conn, theirs, "theirs.only@example.org").expect("query"),
        "and it is still there"
    );
}
