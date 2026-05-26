//! Unified read across the three audit substrates (Item C/W5).
//!
//! `sync_actions` (tier-1 typed app events), `security_events` (tier-2
//! auth events), and `audit_log` (tier-3 trigger JSON diffs) are
//! `UNION ALL`-projected into one [`AuditEntry`] shape so an admin /
//! audit reviewer can ask compliance questions against a single feed.
//!
//! Pagination is keyset on `(occurred_at DESC, tier DESC, row_id DESC)`:
//! `row_id` alone collides across sources (audit_log id 5 and
//! security_events id 5 are unrelated), so `tier` is part of the key.
//!
//! Workspace scoping: `sync_actions` and `audit_log` carry
//! `workspace_id` and are filtered by RLS under the tenant connection.
//! `security_events` is platform-wide (no workspace_id); in the
//! single-tenant V1 build that is the whole instance. Hosted
//! multi-tenant will need a tenant key on security_events before this
//! feed is exposed cross-tenant.

use crate::db::DbConnection;
use crate::repository::audit_log::DiffEntry;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_types::{Array, BigInt, Jsonb, Nullable, SmallInt, Text, Timestamptz};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which substrate an entry came from.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AuditSource {
    /// sync_actions
    Tier1,
    /// security_events
    Tier2,
    /// audit_log
    Tier3,
}

impl AuditSource {
    fn from_tier(tier: i16) -> Self {
        match tier {
            1 => Self::Tier1,
            2 => Self::Tier2,
            _ => Self::Tier3,
        }
    }
}

/// Reference to the entity an entry concerns (tier-1 aggregate,
/// tier-3 table row). Absent for tier-2 auth events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetRef {
    pub kind: String,
    pub id: String,
}

/// One unified audit entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Stable composite key (`"{tier}:{row_id}"`); unique across the
    /// union and used by the client as the row key.
    pub id: String,
    pub source: AuditSource,
    pub occurred_at: DateTime<Utc>,
    pub actor_kind: String,
    pub actor_uuid: Option<Uuid>,
    pub event_type: String,
    pub target: Option<TargetRef>,
    /// Tier-1 `data` / tier-2 `details`. Null for tier-3 (use `diff`).
    pub payload: Option<serde_json::Value>,
    /// Field-level diff for tier-3 rows; empty otherwise.
    pub diff: Vec<DiffEntry>,
    pub correlation_id: Option<Uuid>,
    /// Tier-2 only.
    pub source_ip: Option<String>,
    pub severity: String,
    /// Tier-1 only (sync_actions event_uuid).
    pub event_uuid: Option<Uuid>,
}

/// Server-side filters. All optional.
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    pub actor_uuid: Option<Uuid>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    /// Matches `event_type LIKE '<prefix>%'` (e.g. `auth.`).
    pub event_prefix: Option<String>,
    /// 1, 2, or 3.
    pub tier: Option<i16>,
    pub severity: Option<String>,
}

/// Keyset cursor describing the last row of the previous page.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Cursor {
    pub occurred_at: DateTime<Utc>,
    pub tier: i16,
    pub row_id: i64,
}

/// One page plus the next cursor (None on the last page).
#[derive(Debug)]
pub struct Page {
    pub entries: Vec<AuditEntry>,
    pub next_cursor: Option<Cursor>,
}

/// Raw union row as returned by the SQL projection.
#[derive(QueryableByName)]
struct UnifiedRow {
    #[diesel(sql_type = SmallInt)]
    tier: i16,
    #[diesel(sql_type = BigInt)]
    row_id: i64,
    #[diesel(sql_type = Timestamptz)]
    occurred_at: DateTime<Utc>,
    #[diesel(sql_type = Nullable<diesel::sql_types::Uuid>)]
    actor_uuid: Option<Uuid>,
    #[diesel(sql_type = Text)]
    actor_kind: String,
    #[diesel(sql_type = Text)]
    event_type: String,
    #[diesel(sql_type = Nullable<Text>)]
    target_type: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    target_id: Option<String>,
    #[diesel(sql_type = Nullable<Jsonb>)]
    payload: Option<serde_json::Value>,
    #[diesel(sql_type = Nullable<Jsonb>)]
    before_jsonb: Option<serde_json::Value>,
    #[diesel(sql_type = Nullable<Jsonb>)]
    after_jsonb: Option<serde_json::Value>,
    #[diesel(sql_type = Nullable<Array<Nullable<Text>>>)]
    changed_cols: Option<Vec<Option<String>>>,
    #[diesel(sql_type = Nullable<diesel::sql_types::Uuid>)]
    correlation_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<Text>)]
    source_ip: Option<String>,
    #[diesel(sql_type = Text)]
    severity: String,
    #[diesel(sql_type = Nullable<diesel::sql_types::Uuid>)]
    event_uuid: Option<Uuid>,
}

/// The three-way projection. Column names/types are fixed by the first
/// (tier-1) branch; later branches match by position. Filters and the
/// keyset predicate are applied in the outer query so Postgres can push
/// the time bound down into each partition-pruned branch.
const UNION_SQL: &str = "\
SELECT * FROM ( \
  SELECT 1::smallint AS tier, sync_id::bigint AS row_id, occurred_at, \
         actor_uuid, actor_kind, event_type, \
         aggregate::text AS target_type, aggregate_id AS target_id, \
         data AS payload, NULL::jsonb AS before_jsonb, NULL::jsonb AS after_jsonb, \
         NULL::text[] AS changed_cols, correlation_id, \
         NULL::text AS source_ip, 'info'::text AS severity, event_uuid \
  FROM sync_actions \
  UNION ALL \
  SELECT 2::smallint, id::bigint, created_at, \
         user_uuid, CASE WHEN user_uuid IS NULL THEN 'anonymous' ELSE 'user' END, event_type, \
         NULL::text, NULL::text, \
         details, NULL::jsonb, NULL::jsonb, \
         NULL::text[], NULL::uuid, \
         host(ip_address)::text, severity, NULL::uuid \
  FROM security_events \
  UNION ALL \
  SELECT 3::smallint, id::bigint, occurred_at, \
         actor_uuid, CASE WHEN actor_uuid IS NULL THEN 'system' ELSE 'user' END, \
         table_name || '.' || CASE op WHEN 'I' THEN 'created' WHEN 'U' THEN 'updated' WHEN 'D' THEN 'deleted' ELSE op END, \
         table_name, pk_text, \
         NULL::jsonb, before_jsonb, after_jsonb, \
         changed_cols, correlation_id, \
         NULL::text, 'info'::text, NULL::uuid \
  FROM audit_log \
) u \
WHERE ($1::timestamptz IS NULL OR u.occurred_at >= $1) \
  AND ($2::timestamptz IS NULL OR u.occurred_at <  $2) \
  AND ($3::uuid        IS NULL OR u.actor_uuid = $3) \
  AND ($4::text        IS NULL OR u.event_type LIKE $4 || '%') \
  AND ($5::smallint    IS NULL OR u.tier = $5) \
  AND ($6::text        IS NULL OR u.severity = $6) \
  AND ($7::timestamptz IS NULL OR (u.occurred_at, u.tier, u.row_id) < ($7::timestamptz, $8::smallint, $9::bigint)) \
ORDER BY u.occurred_at DESC, u.tier DESC, u.row_id DESC \
LIMIT $10";

// sync-audit-only: read-only UNION SELECT across the audit substrates; performs no writes (sql_query is a SELECT here)
/// List unified audit entries, newest first, keyset-paginated.
/// `limit` is clamped to [1, 200].
pub fn list(
    conn: &mut DbConnection,
    filter: &AuditFilter,
    cursor: Option<Cursor>,
    limit: i64,
) -> Result<Page, DieselError> {
    let limit = limit.clamp(1, 200);

    let rows: Vec<UnifiedRow> = diesel::sql_query(UNION_SQL)
        .bind::<Nullable<Timestamptz>, _>(filter.since)
        .bind::<Nullable<Timestamptz>, _>(filter.until)
        .bind::<Nullable<diesel::sql_types::Uuid>, _>(filter.actor_uuid)
        .bind::<Nullable<Text>, _>(filter.event_prefix.clone())
        .bind::<Nullable<SmallInt>, _>(filter.tier)
        .bind::<Nullable<Text>, _>(filter.severity.clone())
        .bind::<Nullable<Timestamptz>, _>(cursor.map(|c| c.occurred_at))
        .bind::<Nullable<SmallInt>, _>(cursor.map(|c| c.tier))
        .bind::<Nullable<BigInt>, _>(cursor.map(|c| c.row_id))
        .bind::<BigInt, _>(limit + 1)
        .load(conn)?;

    let mut entries: Vec<AuditEntry> = rows.into_iter().map(into_entry).collect();

    let next_cursor = if entries.len() as i64 > limit {
        entries.pop();
        entries.last().map(|e| Cursor {
            occurred_at: e.occurred_at,
            // Re-derive tier/row_id from the composite id rather than
            // carrying them on AuditEntry: the id is "{tier}:{row_id}".
            tier: tier_of(e),
            row_id: row_id_of(e),
        })
    } else {
        None
    };

    Ok(Page {
        entries,
        next_cursor,
    })
}

fn tier_of(e: &AuditEntry) -> i16 {
    match e.source {
        AuditSource::Tier1 => 1,
        AuditSource::Tier2 => 2,
        AuditSource::Tier3 => 3,
    }
}

fn row_id_of(e: &AuditEntry) -> i64 {
    e.id.split_once(':')
        .and_then(|(_, id)| id.parse().ok())
        .unwrap_or(0)
}

fn into_entry(r: UnifiedRow) -> AuditEntry {
    let source = AuditSource::from_tier(r.tier);
    let target = match (r.target_type, r.target_id) {
        (Some(kind), Some(id)) => Some(TargetRef { kind, id }),
        (Some(kind), None) => Some(TargetRef {
            kind,
            id: String::new(),
        }),
        _ => None,
    };
    let diff = if source == AuditSource::Tier3 {
        flatten(
            r.before_jsonb.as_ref(),
            r.after_jsonb.as_ref(),
            &r.changed_cols,
        )
    } else {
        Vec::new()
    };
    AuditEntry {
        id: format!("{}:{}", r.tier, r.row_id),
        source,
        occurred_at: r.occurred_at,
        actor_kind: r.actor_kind,
        actor_uuid: r.actor_uuid,
        event_type: r.event_type,
        target,
        payload: r.payload,
        diff,
        correlation_id: r.correlation_id,
        source_ip: r.source_ip,
        severity: r.severity,
        event_uuid: r.event_uuid,
    }
}

/// Field-level diff from a tier-3 row's before/after/changed_cols,
/// mirroring `audit_log::flatten_diff` but driven by value presence
/// (the unified projection doesn't carry the op char):
/// before-only => delete, after-only => insert, both => changed_cols.
fn flatten(
    before: Option<&serde_json::Value>,
    after: Option<&serde_json::Value>,
    changed_cols: &Option<Vec<Option<String>>>,
) -> Vec<DiffEntry> {
    match (before, after) {
        (Some(b), Some(a)) => changed_cols
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter_map(|c| c.as_deref())
            .map(|field| DiffEntry {
                field: field.to_string(),
                old: b.get(field).cloned(),
                new: a.get(field).cloned(),
            })
            .collect(),
        (None, Some(a)) => object_entries(a, true),
        (Some(b), None) => object_entries(b, false),
        (None, None) => Vec::new(),
    }
}

fn object_entries(v: &serde_json::Value, is_new: bool) -> Vec<DiffEntry> {
    v.as_object()
        .map(|obj| {
            obj.iter()
                .map(|(k, val)| DiffEntry {
                    field: k.clone(),
                    old: if is_new { None } else { Some(val.clone()) },
                    new: if is_new { Some(val.clone()) } else { None },
                })
                .collect()
        })
        .unwrap_or_default()
}
