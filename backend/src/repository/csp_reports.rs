//! CSP violation report storage.
//!
//! Reports come in over the public `/api/csp-report` endpoint and
//! land here. Duplicates are absorbed into a running occurrence
//! count rather than stored as separate rows — without this, a
//! single broken page reload could explode the table by hundreds
//! of rows in seconds.

use crate::db::DbConnection;
use crate::models::{CspReport, NewCspReport};
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use ring::digest::{Context, SHA256};

/// Compute the canonical deduplication hash for a report. Reports
/// that produce the same hash are treated as the same violation
/// recurring, even if document_uri / referrer / user_agent differ
/// between submissions. The dedup tuple is intentionally narrow:
/// (effective_directive, blocked_uri, source_file, line_number).
/// Adding more fields makes dedup less aggressive and inflates
/// row count for what's effectively the same violation seen from
/// many pages.
pub fn dedup_hash(
    effective_directive: &str,
    blocked_uri: Option<&str>,
    source_file: Option<&str>,
    line_number: Option<i32>,
) -> String {
    let mut h = Context::new(&SHA256);
    h.update(effective_directive.as_bytes());
    h.update(b"|");
    h.update(blocked_uri.unwrap_or("").as_bytes());
    h.update(b"|");
    h.update(source_file.unwrap_or("").as_bytes());
    h.update(b"|");
    h.update(line_number.unwrap_or(0).to_string().as_bytes());
    let digest = h.finish();
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

// sync-audit-only: CSP violation ingest; operational security log, no sync subscriber
/// Insert a report, or if its dedup hash collides with an existing
/// row, increment occurrence_count and bump last_seen_at on the
/// existing row. ON CONFLICT keeps this single-roundtrip; a
/// SELECT-then-UPDATE-or-INSERT pattern would race with concurrent
/// reports and either lose increments or produce duplicates.
pub fn upsert(conn: &mut DbConnection, report: NewCspReport) -> Result<CspReport, DieselError> {
    use crate::schema::csp_reports::dsl::*;
    use diesel::dsl::now;
    use diesel::pg::upsert::excluded;

    diesel::insert_into(csp_reports)
        .values(&report)
        .on_conflict(dedup_hash)
        .do_update()
        .set((
            occurrence_count.eq(occurrence_count + 1),
            last_seen_at.eq(now),
            // Refresh context columns to whatever was most-recently
            // observed. Doesn't change the dedup identity but lets
            // the admin UI surface up-to-date document_uri /
            // user_agent for the latest occurrence.
            document_uri.eq(excluded(document_uri)),
            referrer.eq(excluded(referrer)),
            user_agent.eq(excluded(user_agent)),
            user_uuid.eq(excluded(user_uuid)),
        ))
        .get_result::<CspReport>(conn)
}

/// Aggregate view for the admin UI. Most-recently-seen first,
/// limited to a window. Plenty for "what's broken right now"
/// triage; older data is queryable via raw SQL when needed.
pub fn list_recent(conn: &mut DbConnection, limit: i64) -> Result<Vec<CspReport>, DieselError> {
    use crate::schema::csp_reports::dsl::*;

    csp_reports
        .order(last_seen_at.desc())
        .limit(limit)
        .load::<CspReport>(conn)
}

// sync-audit-only: retention sweep on the CSP report table
/// Drop reports whose last_seen_at is older than `older_than_days`.
/// Called from the scheduler. Returns the row count deleted so the
/// caller can log meaningful "pruned 312 reports" output instead of
/// silent.
pub fn prune_older_than(
    conn: &mut DbConnection,
    older_than_days: i32,
) -> Result<usize, DieselError> {
    use crate::schema::csp_reports::dsl::*;
    use diesel::dsl::sql;
    use diesel::sql_types::Timestamptz;

    let cutoff = sql::<Timestamptz>(&format!("NOW() - INTERVAL '{older_than_days} days'"));
    diesel::delete(csp_reports.filter(last_seen_at.lt(cutoff))).execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_hash_is_stable_per_tuple() {
        let a = dedup_hash("script-src", Some("https://evil.com/x.js"), None, None);
        let b = dedup_hash("script-src", Some("https://evil.com/x.js"), None, None);
        assert_eq!(a, b);
    }

    #[test]
    fn dedup_hash_distinguishes_directive() {
        let a = dedup_hash("script-src", Some("https://x.com/y"), None, None);
        let b = dedup_hash("img-src", Some("https://x.com/y"), None, None);
        assert_ne!(a, b);
    }

    #[test]
    fn dedup_hash_distinguishes_blocked_uri() {
        let a = dedup_hash("script-src", Some("https://a.com/x"), None, None);
        let b = dedup_hash("script-src", Some("https://b.com/x"), None, None);
        assert_ne!(a, b);
    }

    #[test]
    fn dedup_hash_treats_none_blocked_uri_consistently() {
        let a = dedup_hash("script-src", None, None, None);
        let b = dedup_hash("script-src", Some(""), None, None);
        // Empty string and None should hash the same — both mean
        // "no blocked URI was reported." Tests pin this so an
        // accidental Some("") vs None split doesn't fragment dedup.
        assert_eq!(a, b);
    }

    // ---- Phase 3i.7: composite UNIQUE invariant ----

    #[test]
    fn unique_index_allows_cross_tenant_collision_blocks_intra_tenant() {
        // The 3i.2 migration flipped csp_reports_dedup_hash_idx
        // from UNIQUE (dedup_hash) to UNIQUE (workspace_id,
        // dedup_hash). Two tenants reporting the same browser-
        // computed dedup_hash must coexist (cross-tenant
        // collisions are silent under the old shape and corrupt
        // one tenant's row into the other via upsert). Inside one
        // tenant the constraint must still prevent duplicate rows.
        // This test exercises both halves of the invariant
        // directly via raw SQL so it fails loudly if a future
        // migration narrows the index back.
        use crate::sync::actor::ActorContext;
        use crate::sync::session::with_actor_bypass_context;
        use crate::test_helpers::setup_test_connection;

        let mut conn = setup_test_connection();
        let admin = ActorContext::system("rls.test");

        // Seed workspace 2 (workspace 1 already exists).
        with_actor_bypass_context(&mut conn, &admin, |c| {
            diesel::sql_query(
                "INSERT INTO workspaces (id, slug, name) \
                 VALUES (2, 'csp-other', 'CSP Other Workspace') \
                 ON CONFLICT (id) DO NOTHING",
            )
            .execute(c)?;
            diesel::sql_query("SELECT setval('workspaces_id_seq', GREATEST(2, last_value), true) FROM workspaces_id_seq")
                .execute(c)?;
            Ok::<(), diesel::result::Error>(())
        })
        .expect("seed workspace 2");

        let hash_a = "a".repeat(64);

        // Insert ws1, hash_a — first occurrence in workspace 1.
        let cross_results = with_actor_bypass_context(&mut conn, &admin, |c| {
            diesel::sql_query(
                "INSERT INTO csp_reports \
                    (workspace_id, dedup_hash, effective_directive, \
                     document_uri, disposition) \
                 VALUES (1, $1, 'script-src', 'https://a.example/', 'enforce')",
            )
            .bind::<diesel::sql_types::Text, _>(&hash_a)
            .execute(c)?;

            // Same hash in workspace 2 — must succeed under the
            // composite shape, fail under the narrow shape.
            diesel::sql_query(
                "INSERT INTO csp_reports \
                    (workspace_id, dedup_hash, effective_directive, \
                     document_uri, disposition) \
                 VALUES (2, $1, 'script-src', 'https://b.example/', 'enforce')",
            )
            .bind::<diesel::sql_types::Text, _>(&hash_a)
            .execute(c)
        });

        assert!(
            cross_results.is_ok(),
            "cross-tenant duplicate dedup_hash must be permitted by the composite UNIQUE; \
             if this fails, the index was narrowed back to (dedup_hash) only"
        );

        // Intra-tenant duplicate must still error.
        let intra_result = with_actor_bypass_context(&mut conn, &admin, |c| {
            diesel::sql_query(
                "INSERT INTO csp_reports \
                    (workspace_id, dedup_hash, effective_directive, \
                     document_uri, disposition) \
                 VALUES (1, $1, 'script-src', 'https://a.example/again', 'enforce')",
            )
            .bind::<diesel::sql_types::Text, _>(&hash_a)
            .execute(c)
        });

        assert!(
            intra_result.is_err(),
            "intra-tenant duplicate (workspace_id, dedup_hash) must violate the UNIQUE index"
        );
    }
}
