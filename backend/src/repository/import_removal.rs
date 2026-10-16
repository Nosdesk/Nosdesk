//! Reads and writes for Remove imported assets
//! (`services::imports::remove`): the state of the assets an import
//! created, everything that can have touched them since, and the catalog
//! entries it created. Every read takes a list of ids and answers in one
//! query, so a 5,000-asset import is a dozen queries, not 60,000.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::dsl::{count_star, max, min};
use diesel::prelude::*;
use diesel::sql_types::{Array, Int4, Text, Timestamptz};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::schema::{
    asset_audits, asset_directory_memberships, asset_disposals, asset_group_assignments,
    asset_groups, asset_lifecycle_events, asset_loans, asset_media, asset_models, asset_usage_log,
    assets, manufacturers, ticket_assets,
};

/// What the untouched rule reads off an asset row itself.
#[derive(Debug, Clone, Queryable)]
pub struct AssetState {
    pub id: i32,
    pub name: String,
    pub asset_tag: Option<String>,
    pub updated_at: NaiveDateTime,
    pub external_sync_source: Option<String>,
}

/// The assets of `ids` that still exist, by id. With `lock`, each row is
/// locked `FOR UPDATE` in id order for the rest of the transaction: the
/// rows that pass are deleted, which takes that lock anyway, and taking it
/// before the checks keeps a loan, a ticket link or any other child row
/// from landing between the check and the delete.
// sync-audit-only: read-only (takes row locks, writes nothing)
pub fn asset_states(
    conn: &mut DbConnection,
    ids: &[i32],
    lock: bool,
) -> QueryResult<Vec<AssetState>> {
    let query = assets::table
        .filter(assets::id.eq_any(ids))
        .order(assets::id.asc())
        .select((
            assets::id,
            assets::name,
            assets::asset_tag,
            assets::updated_at,
            assets::external_sync_source,
        ));
    if lock {
        query.for_update().load(conn)
    } else {
        query.load(conn)
    }
}

/// Each asset's newest lifecycle event: the highest id, so an event
/// recorded later with an earlier `occurred_at` still counts as later.
// sync-audit-only: read-only
pub fn newest_lifecycle_events(
    conn: &mut DbConnection,
    ids: &[i32],
) -> QueryResult<HashMap<i32, i32>> {
    let rows: Vec<(i32, Option<i32>)> = asset_lifecycle_events::table
        .filter(asset_lifecycle_events::asset_id.eq_any(ids))
        .group_by(asset_lifecycle_events::asset_id)
        .select((
            asset_lifecycle_events::asset_id,
            max(asset_lifecycle_events::id),
        ))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .filter_map(|(asset, id)| Some((asset, id?)))
        .collect())
}

/// How many loan records each asset has, returned or not.
// sync-audit-only: read-only
pub fn loan_counts(conn: &mut DbConnection, ids: &[i32]) -> QueryResult<HashMap<i32, i64>> {
    asset_loans::table
        .filter(asset_loans::asset_id.eq_any(ids))
        .group_by(asset_loans::asset_id)
        .select((asset_loans::asset_id, count_star()))
        .load::<(i32, i64)>(conn)
        .map(|rows| rows.into_iter().collect())
}

/// How many disposal records each asset has.
// sync-audit-only: read-only
pub fn disposal_counts(conn: &mut DbConnection, ids: &[i32]) -> QueryResult<HashMap<i32, i64>> {
    asset_disposals::table
        .filter(asset_disposals::asset_id.eq_any(ids))
        .group_by(asset_disposals::asset_id)
        .select((asset_disposals::asset_id, count_star()))
        .load::<(i32, i64)>(conn)
        .map(|rows| rows.into_iter().collect())
}

/// Each asset's photos and other files: `(photos, files)`.
// sync-audit-only: read-only
pub fn media_counts(conn: &mut DbConnection, ids: &[i32]) -> QueryResult<HashMap<i32, (i64, i64)>> {
    let rows: Vec<(i32, String, i64)> = asset_media::table
        .filter(asset_media::asset_id.eq_any(ids))
        .group_by((asset_media::asset_id, asset_media::kind))
        .select((asset_media::asset_id, asset_media::kind, count_star()))
        .load(conn)?;
    let mut out: HashMap<i32, (i64, i64)> = HashMap::new();
    for (asset, kind, n) in rows {
        let entry = out.entry(asset).or_default();
        if kind == "photo" {
            entry.0 += n;
        } else {
            entry.1 += n;
        }
    }
    Ok(out)
}

/// Each audited asset's latest count.
// sync-audit-only: read-only
pub fn latest_audits(
    conn: &mut DbConnection,
    ids: &[i32],
) -> QueryResult<HashMap<i32, DateTime<Utc>>> {
    let rows: Vec<(i32, Option<DateTime<Utc>>)> = asset_audits::table
        .filter(asset_audits::asset_id.eq_any(ids))
        .group_by(asset_audits::asset_id)
        .select((asset_audits::asset_id, max(asset_audits::recorded_at)))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .filter_map(|(asset, at)| Some((asset, at?)))
        .collect())
}

/// How many usage entries each asset has.
// sync-audit-only: read-only
pub fn usage_counts(conn: &mut DbConnection, ids: &[i32]) -> QueryResult<HashMap<i32, i64>> {
    asset_usage_log::table
        .filter(asset_usage_log::asset_id.eq_any(ids))
        .group_by(asset_usage_log::asset_id)
        .select((asset_usage_log::asset_id, count_star()))
        .load::<(i32, i64)>(conn)
        .map(|rows| rows.into_iter().collect())
}

/// The groups each asset is assigned to, archived ones included.
// sync-audit-only: read-only
pub fn group_assignments(
    conn: &mut DbConnection,
    ids: &[i32],
) -> QueryResult<HashMap<i32, Vec<i32>>> {
    let rows: Vec<(i32, i32)> = asset_group_assignments::table
        .filter(asset_group_assignments::asset_id.eq_any(ids))
        .select((
            asset_group_assignments::asset_id,
            asset_group_assignments::group_id,
        ))
        .load(conn)?;
    let mut out: HashMap<i32, Vec<i32>> = HashMap::new();
    for (asset, group) in rows {
        out.entry(asset).or_default().push(group);
    }
    Ok(out)
}

/// The lowest ticket each linked asset is linked to.
// sync-audit-only: read-only
pub fn linked_tickets(conn: &mut DbConnection, ids: &[i32]) -> QueryResult<HashMap<i32, i32>> {
    let rows: Vec<(i32, Option<i32>)> = ticket_assets::table
        .filter(ticket_assets::asset_id.eq_any(ids))
        .group_by(ticket_assets::asset_id)
        .select((ticket_assets::asset_id, min(ticket_assets::ticket_id)))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .filter_map(|(asset, ticket)| Some((asset, ticket?)))
        .collect())
}

/// How many directory groups each asset is a member of.
// sync-audit-only: read-only
pub fn directory_counts(conn: &mut DbConnection, ids: &[i32]) -> QueryResult<HashMap<i32, i64>> {
    asset_directory_memberships::table
        .filter(asset_directory_memberships::asset_id.eq_any(ids))
        .group_by(asset_directory_memberships::asset_id)
        .select((asset_directory_memberships::asset_id, count_star()))
        .load::<(i32, i64)>(conn)
        .map(|rows| rows.into_iter().collect())
}

/// An attribute value on one asset that names another asset's id.
#[derive(Debug, Clone, QueryableByName)]
pub struct AttributeReference {
    #[diesel(sql_type = Int4)]
    pub referrer_id: i32,
    #[diesel(sql_type = Text)]
    pub referrer_name: String,
    #[diesel(sql_type = Text)]
    pub referrer_kind: String,
    #[diesel(sql_type = Text)]
    pub key: String,
    #[diesel(sql_type = Text)]
    pub value: String,
}

/// Every attribute, among `keys`, whose value is one of `targets` (asset
/// ids as text). The caller keeps those whose key is an asset reference
/// on the referring asset's kind.
// sync-audit-only: read-only
pub fn attribute_references(
    conn: &mut DbConnection,
    keys: &[String],
    targets: &[String],
) -> QueryResult<Vec<AttributeReference>> {
    if keys.is_empty() || targets.is_empty() {
        return Ok(Vec::new());
    }
    diesel::sql_query(
        "SELECT a.id AS referrer_id, a.name::text AS referrer_name, a.kind::text AS referrer_kind, \
                kv.key, kv.value \
         FROM (SELECT id, name, kind, attributes FROM assets \
               WHERE jsonb_typeof(attributes) = 'object' AND attributes ?| $1) a \
         CROSS JOIN LATERAL jsonb_each_text(a.attributes) kv \
         WHERE kv.key = ANY($1) AND kv.value = ANY($2)",
    )
    .bind::<Array<Text>, _>(keys)
    .bind::<Array<Text>, _>(targets)
    .load(conn)
}

#[derive(QueryableByName)]
struct LastEdit {
    #[diesel(sql_type = Text)]
    pk_text: String,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Uuid>)]
    actor_uuid: Option<Uuid>,
}

/// Who last changed each of `ids` after `since`, from the audit log;
/// absent when the change has no actor or isn't logged.
// sync-audit-only: read-only
pub fn last_editors(
    conn: &mut DbConnection,
    ids: &[i32],
    since: DateTime<Utc>,
) -> QueryResult<HashMap<i32, Uuid>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let pks: Vec<String> = ids.iter().map(ToString::to_string).collect();
    let rows: Vec<LastEdit> = diesel::sql_query(
        "SELECT DISTINCT ON (pk_text) pk_text, actor_uuid FROM audit_log \
         WHERE table_name = 'assets' AND op = 'U' AND pk_text = ANY($1) AND occurred_at > $2 \
         ORDER BY pk_text, occurred_at DESC, id DESC",
    )
    .bind::<Array<Text>, _>(&pks)
    .bind::<Timestamptz, _>(since)
    .load(conn)?;
    Ok(rows
        .into_iter()
        .filter_map(|r| Some((r.pk_text.parse().ok()?, r.actor_uuid?)))
        .collect())
}

/// A catalog entry's `updated_at` (and, for a group, `archived_at`).
#[derive(Debug, Clone, Copy)]
pub struct CatalogState {
    pub id: i32,
    pub updated_at: NaiveDateTime,
    pub archived: bool,
}

/// The manufacturers of `ids` that still exist. With `lock`, `FOR UPDATE`
/// in id order: those nothing uses are deleted.
// sync-audit-only: read-only (takes row locks, writes nothing)
pub fn manufacturer_states(
    conn: &mut DbConnection,
    ids: &[i32],
    lock: bool,
) -> QueryResult<Vec<CatalogState>> {
    let query = manufacturers::table
        .filter(manufacturers::id.eq_any(ids))
        .order(manufacturers::id.asc())
        .select((manufacturers::id, manufacturers::updated_at));
    let rows: Vec<(i32, NaiveDateTime)> = if lock {
        query.for_update().load(conn)?
    } else {
        query.load(conn)?
    };
    Ok(rows
        .into_iter()
        .map(|(id, updated_at)| CatalogState {
            id,
            updated_at,
            archived: false,
        })
        .collect())
}

/// The models of `ids` that still exist. With `lock`, `FOR UPDATE` in id
/// order: those nothing uses are deleted.
// sync-audit-only: read-only (takes row locks, writes nothing)
pub fn model_states(
    conn: &mut DbConnection,
    ids: &[i32],
    lock: bool,
) -> QueryResult<Vec<CatalogState>> {
    let query = asset_models::table
        .filter(asset_models::id.eq_any(ids))
        .order(asset_models::id.asc())
        .select((asset_models::id, asset_models::updated_at));
    let rows: Vec<(i32, NaiveDateTime)> = if lock {
        query.for_update().load(conn)?
    } else {
        query.load(conn)?
    };
    Ok(rows
        .into_iter()
        .map(|(id, updated_at)| CatalogState {
            id,
            updated_at,
            archived: false,
        })
        .collect())
}

/// The groups of `ids` that still exist. With `lock`, `FOR UPDATE` in id
/// order: those with no assignment are deleted, the others archived.
// sync-audit-only: read-only (takes row locks, writes nothing)
pub fn group_states(
    conn: &mut DbConnection,
    ids: &[i32],
    lock: bool,
) -> QueryResult<Vec<CatalogState>> {
    let query = asset_groups::table
        .filter(asset_groups::id.eq_any(ids))
        .order(asset_groups::id.asc())
        .select((
            asset_groups::id,
            asset_groups::updated_at,
            asset_groups::archived_at,
        ));
    let rows: Vec<(i32, NaiveDateTime, Option<NaiveDateTime>)> = if lock {
        query.for_update().load(conn)?
    } else {
        query.load(conn)?
    };
    Ok(rows
        .into_iter()
        .map(|(id, updated_at, archived_at)| CatalogState {
            id,
            updated_at,
            archived: archived_at.is_some(),
        })
        .collect())
}

/// Of the models `ids`, those an asset is linked to.
// sync-audit-only: read-only
pub fn models_in_use(conn: &mut DbConnection, ids: &[i32]) -> QueryResult<Vec<i32>> {
    assets::table
        .filter(assets::model_id.eq_any(ids))
        .select(assets::model_id.assume_not_null())
        .distinct()
        .load(conn)
}

/// Of the manufacturers `ids`, those a model belongs to.
// sync-audit-only: read-only
pub fn manufacturers_in_use(conn: &mut DbConnection, ids: &[i32]) -> QueryResult<Vec<i32>> {
    asset_models::table
        .filter(asset_models::manufacturer_id.eq_any(ids))
        .select(asset_models::manufacturer_id)
        .distinct()
        .load(conn)
}

/// Of the groups `ids`, those an asset is assigned to.
// sync-audit-only: read-only
pub fn groups_in_use(conn: &mut DbConnection, ids: &[i32]) -> QueryResult<Vec<i32>> {
    asset_group_assignments::table
        .filter(asset_group_assignments::group_id.eq_any(ids))
        .select(asset_group_assignments::group_id)
        .distinct()
        .load(conn)
}
