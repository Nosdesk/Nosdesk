//! W5b integration: the unified audit feed spans all three substrates,
//! keyset pagination is gap-free, and the D5 recursive-read guard stops
//! an audit-read transaction from generating new audit_log rows.

mod common;

use common::{insert_user, TestDb};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};

use backend::models::{SyncAggregate, SyncOp};
use backend::repository::audit::{self, AuditFilter, AuditSource};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn audit_log_count(conn: &mut PgConnection, table: &str) -> i64 {
    diesel::sql_query("SELECT count(*) AS n FROM audit_log WHERE table_name = $1")
        .bind::<Text, _>(table)
        .get_result::<Count>(conn)
        .expect("count audit_log")
        .n
}

#[test]
fn recursive_read_guard_suppresses_audit_rows() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let actor = ActorContext::user(uuid::Uuid::now_v7(), None);

    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |conn| {
        // Control: a write with the guard unset produces an audit row.
        let before = audit_log_count(conn, "users");
        insert_user(conn, "guard-control");
        let after_unguarded = audit_log_count(conn, "users");
        assert_eq!(
            after_unguarded,
            before + 1,
            "an unguarded insert should write one audit_log row"
        );

        // With the D5 guard armed, further writes to audited tables
        // produce no audit rows.
        diesel::sql_query("SET LOCAL nosdesk.in_audit_read = 'true'").execute(conn)?;
        insert_user(conn, "guard-suppressed");
        let after_guarded = audit_log_count(conn, "users");
        assert_eq!(
            after_guarded, after_unguarded,
            "writes inside an audit-read transaction must not be audited"
        );
        Ok(())
    })
    .expect("with_actor_context");
}

#[test]
fn unified_list_spans_all_three_tiers() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let actor = ActorContext::user(uuid::Uuid::now_v7(), None);

    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |conn| {
        // Tier-3: a row diff via the audit trigger.
        insert_user(conn, "tier3-target");

        // Tier-2: an auth security event.
        backend::utils::security_events::record_security_event(
            conn,
            backend::utils::security_events::SecurityEventInput {
                user_uuid: None,
                event_type: "auth.login.failure",
                severity: "warning",
                details: Some(serde_json::json!({ "reason": "bad_password" })),
                request: None,
                session_id: None,
            },
        )?;

        // Tier-1: a typed app event.
        backend::sync::emit::record(
            conn,
            backend::sync::emit::SyncEmit {
                aggregate: SyncAggregate::Data,
                aggregate_id: "audit".to_string(),
                op: SyncOp::Insert,
                event_type: "data.audit.read",
                data: serde_json::json!({ "rows_returned": 0 }),
                groups: backend::sync::groups::workspace(),
                causation_id: None,
            },
        )?;

        let page = audit::list(conn, &AuditFilter::default(), None, 100)?;
        let sources: std::collections::HashSet<_> = page.entries.iter().map(|e| e.source).collect();

        assert!(
            sources.contains(&AuditSource::Tier1),
            "expected a tier-1 entry"
        );
        assert!(
            sources.contains(&AuditSource::Tier2),
            "expected a tier-2 entry"
        );
        assert!(
            sources.contains(&AuditSource::Tier3),
            "expected a tier-3 entry"
        );

        // The tier-2 entry carries its event_type and severity; the
        // tier-3 entry carries a diff and a target.
        let t2 = page
            .entries
            .iter()
            .find(|e| e.source == AuditSource::Tier2)
            .unwrap();
        assert_eq!(t2.event_type, "auth.login.failure");
        assert_eq!(t2.severity, "warning");

        let t3 = page
            .entries
            .iter()
            .find(|e| e.source == AuditSource::Tier3)
            .unwrap();
        assert!(t3.target.is_some(), "tier-3 entry should have a target");
        assert!(!t3.diff.is_empty(), "tier-3 entry should carry a diff");

        Ok(())
    })
    .expect("with_actor_context");
}

#[test]
fn actor_name_resolves_to_acting_user() {
    let db = TestDb::new();
    let mut conn = db.conn();

    // Seed the actor under a bootstrap context so the users row exists,
    // then act AS that user and emit a tier-1 event.
    let bootstrap = ActorContext::user(uuid::Uuid::now_v7(), None);
    let user = with_actor_context::<_, diesel::result::Error>(&mut conn, &bootstrap, |conn| {
        Ok(insert_user(conn, "Ada Lovelace"))
    })
    .expect("seed user");

    let actor = ActorContext::user(user.uuid, None);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |conn| {
        backend::sync::emit::record(
            conn,
            backend::sync::emit::SyncEmit {
                aggregate: SyncAggregate::Data,
                aggregate_id: "audit".to_string(),
                op: SyncOp::Insert,
                event_type: "data.audit.read",
                data: serde_json::json!({ "rows_returned": 0 }),
                groups: backend::sync::groups::workspace(),
                causation_id: None,
            },
        )?;

        let page = audit::list(conn, &AuditFilter::default(), None, 100)?;
        let entry = page
            .entries
            .iter()
            .find(|e| e.actor_uuid == Some(user.uuid))
            .expect("an entry attributed to the acting user");
        assert_eq!(
            entry.actor_name.as_deref(),
            Some("Ada Lovelace"),
            "the actor uuid should resolve to the user's display name"
        );
        Ok(())
    })
    .expect("with_actor_context");
}

#[test]
fn keyset_pagination_is_gap_free() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let actor = ActorContext::user(uuid::Uuid::now_v7(), None);

    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |conn| {
        for i in 0..5 {
            insert_user(conn, &format!("page-user-{i}"));
        }

        // Walk the feed one entry at a time and confirm no id repeats
        // and the walk terminates.
        let mut seen: Vec<String> = Vec::new();
        let mut cursor = None;
        loop {
            let page = audit::list(conn, &AuditFilter::default(), cursor, 1)?;
            for e in &page.entries {
                assert!(!seen.contains(&e.id), "id {} returned twice", e.id);
                seen.push(e.id.clone());
            }
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
            assert!(seen.len() < 1000, "pagination did not terminate");
        }

        assert!(
            seen.len() >= 5,
            "expected at least the 5 inserted users' audit rows, got {}",
            seen.len()
        );
        Ok(())
    })
    .expect("with_actor_context");
}
