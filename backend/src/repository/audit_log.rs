//! Read-only access to the `audit_log` table.
//!
//! Rows are written by the Postgres trigger `audit_log_trigger()` from
//! inside user-facing transactions; this module never inserts. The
//! query plans here are tuned to hit the existing indexes:
//!
//! * `(table_name, pk_text, occurred_at DESC)` — for "what happened to
//!   this entity" queries (the per-row history surface in the UI).
//! * `BRIN(occurred_at)` — for time-range scans across all entities.
//! * `(actor_uuid, occurred_at DESC) WHERE actor_uuid IS NOT NULL` —
//!   for "what did this user do" investigations.
//!
//! Pagination uses keyset cursors on `(occurred_at DESC, id DESC)`
//! rather than OFFSET; partitioned-table OFFSETs degrade quickly as
//! Postgres has to skip rows in each partition.

use crate::db::DbConnection;
use crate::models::AuditLogRow;
use crate::schema::audit_log;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Filter set accepted by [`list`]. All fields are optional; combining
/// `table_name` + `pk_text` is the single most common shape (e.g. the
/// per-ticket "view history" call).
#[derive(Debug, Clone, Default)]
pub struct AuditLogFilter {
    pub table_name: Option<String>,
    pub pk_text: Option<String>,
    pub actor_uuid: Option<Uuid>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}

/// Cursor describing the last row of the previous page. The next page
/// returns rows strictly less than `(occurred_at, id)`. Cursors are
/// opaque to the client (they're serialized as JSON in the API surface).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Cursor {
    pub occurred_at: DateTime<Utc>,
    pub id: i64,
}

/// One page of audit log rows + (optional) cursor for the next page.
#[derive(Debug)]
pub struct Page {
    pub rows: Vec<AuditLogRow>,
    pub next_cursor: Option<Cursor>,
}

/// List audit-log rows matching `filter`, ordered newest-first, paginated
/// by keyset cursor. `limit` is clamped to [1, 200].
pub fn list(
    conn: &mut DbConnection,
    filter: &AuditLogFilter,
    cursor: Option<Cursor>,
    limit: i64,
) -> Result<Page, DieselError> {
    let limit = limit.clamp(1, 200);

    let mut q = audit_log::table.into_boxed();

    if let Some(t) = &filter.table_name {
        q = q.filter(audit_log::table_name.eq(t));
    }
    if let Some(pk) = &filter.pk_text {
        q = q.filter(audit_log::pk_text.eq(pk));
    }
    if let Some(a) = filter.actor_uuid {
        q = q.filter(audit_log::actor_uuid.eq(a));
    }
    if let Some(since) = filter.since {
        q = q.filter(audit_log::occurred_at.ge(since));
    }
    if let Some(until) = filter.until {
        q = q.filter(audit_log::occurred_at.lt(until));
    }
    if let Some(c) = cursor {
        // Strict tuple comparison: (occurred_at, id) < (cursor.occurred_at, cursor.id).
        // Diesel can't express (a, b) < (x, y) directly so spell out the lexicographic
        // form. Postgres collapses this into a single index seek when the cursor matches
        // the (table_name, pk_text, occurred_at DESC) index lead columns.
        q = q.filter(
            audit_log::occurred_at
                .lt(c.occurred_at)
                .or(audit_log::occurred_at
                    .eq(c.occurred_at)
                    .and(audit_log::id.lt(c.id))),
        );
    }

    let rows: Vec<AuditLogRow> = q
        .order((audit_log::occurred_at.desc(), audit_log::id.desc()))
        .limit(limit + 1)
        .load(conn)?;

    let (rows, next_cursor) = paginate(rows, limit);
    Ok(Page { rows, next_cursor })
}

/// Compute a flattened diff for one row: a list of `(field, old, new)`
/// triples drawn from `changed_cols`. INSERTs report every after-field
/// with `old = null`; DELETEs report every before-field with `new = null`.
/// Returns an empty Vec when the row carries no useful diff (rare; mostly
/// trigger edge cases).
pub fn flatten_diff(row: &AuditLogRow) -> Vec<DiffEntry> {
    match row.op.as_str() {
        "U" => {
            let cols = row.changed_cols.as_deref().unwrap_or(&[]);
            let before = row.before_jsonb.as_ref();
            let after = row.after_jsonb.as_ref();
            cols.iter()
                .filter_map(|c| c.as_deref())
                .map(|field| DiffEntry {
                    field: field.to_string(),
                    old: before.and_then(|v| v.get(field)).cloned(),
                    new: after.and_then(|v| v.get(field)).cloned(),
                })
                .collect()
        }
        "I" => row
            .after_jsonb
            .as_ref()
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| DiffEntry {
                        field: k.clone(),
                        old: None,
                        new: Some(v.clone()),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        "D" => row
            .before_jsonb
            .as_ref()
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| DiffEntry {
                        field: k.clone(),
                        old: Some(v.clone()),
                        new: None,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// One field-level change extracted from an [`AuditLogRow`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffEntry {
    pub field: String,
    pub old: Option<serde_json::Value>,
    pub new: Option<serde_json::Value>,
}

/// `list` queries `limit + 1` rows so we can tell "this page is full and
/// there's more" from "this page is the tail". When the over-fetch is
/// present, drop it and emit a cursor pointing at the new last row.
fn paginate(mut rows: Vec<AuditLogRow>, limit: i64) -> (Vec<AuditLogRow>, Option<Cursor>) {
    if rows.len() as i64 > limit {
        rows.pop();
        let last = rows.last().expect("limit clamped >= 1");
        let cursor = Cursor {
            occurred_at: last.occurred_at,
            id: last.id,
        };
        (rows, Some(cursor))
    } else {
        (rows, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
    use crate::sync::actor::ActorContext;
    use crate::sync::session::with_actor_context;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    /// The trigger writes synchronously inside the user transaction, so a
    /// `with_actor_context` block that creates a user produces an audit_log
    /// row attributed to the actor before we even leave the closure.
    #[test]
    fn list_finds_rows_for_audited_table() {
        let mut conn = setup_test_connection();
        let actor_uuid = uuid::Uuid::now_v7();
        let correlation_id = uuid::Uuid::now_v7();
        let actor = ActorContext::user(actor_uuid, Some(correlation_id));

        let new_user_uuid = with_actor_context::<_, DieselError>(&mut conn, &actor, |conn| {
            let u = TestFixtures::create_user(conn, "audit-target", UserRole::User);
            Ok(u.uuid)
        })
        .expect("with_actor_context succeeded");

        let page = list(
            &mut conn,
            &AuditLogFilter {
                table_name: Some("users".into()),
                pk_text: Some(new_user_uuid.to_string()),
                ..Default::default()
            },
            None,
            10,
        )
        .expect("list");

        assert!(!page.rows.is_empty(), "expected an INSERT audit row");
        let row = &page.rows[0];
        assert_eq!(row.op, "I");
        assert_eq!(row.actor_uuid, Some(actor_uuid));
        assert_eq!(row.correlation_id, Some(correlation_id));
    }

    #[test]
    fn flatten_diff_lists_changed_fields_on_update() {
        let row = AuditLogRow {
            id: 1,
            table_name: "tickets".into(),
            pk_text: "42".into(),
            op: "U".into(),
            before_jsonb: Some(serde_json::json!({"title": "old", "status": "open"})),
            after_jsonb: Some(serde_json::json!({"title": "new", "status": "open"})),
            changed_cols: Some(vec![Some("title".into())]),
            actor_uuid: None,
            correlation_id: None,
            occurred_at: Utc::now(),
        };

        let diff = flatten_diff(&row);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].field, "title");
        assert_eq!(diff[0].old, Some(serde_json::json!("old")));
        assert_eq!(diff[0].new, Some(serde_json::json!("new")));
    }

    #[test]
    fn flatten_diff_inserts_have_null_old() {
        let row = AuditLogRow {
            id: 1,
            table_name: "tickets".into(),
            pk_text: "42".into(),
            op: "I".into(),
            before_jsonb: None,
            after_jsonb: Some(serde_json::json!({"title": "fresh", "id": 42})),
            changed_cols: None,
            actor_uuid: None,
            correlation_id: None,
            occurred_at: Utc::now(),
        };

        let diff = flatten_diff(&row);
        assert_eq!(diff.len(), 2);
        assert!(diff.iter().all(|d| d.old.is_none()));
    }
}
