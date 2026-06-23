//! Native asset groups repository.
//!
//! Workspace-local, user-managed classification of assets ("Loaner pool",
//! "Exec laptops", …). Tag-style: multi-assign, assigned from the asset side.
//! Distinct from `repository::groups`' directory memberships, which Intune/
//! Entra sync owns.
//!
//! Group rows themselves are NOT a sync aggregate — workspace config changes
//! infrequently and the picker re-fetches on demand (same call the tags
//! repository makes). Asset↔group *assignments* DO surface on the asset's
//! sync_actions stream as `asset.groups_changed`, so list / detail views
//! refresh when an asset's groups change.

use diesel::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    Asset, AssetGroup, AssetGroupRef, AssetGroupResponse, AssetGroupUpdate, NewAssetGroup,
    NewAssetGroupAssignment, SyncAggregate, SyncOp,
};
use crate::schema::{asset_group_assignments, asset_groups, assets};
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups as sync_groups;

// ---- Group CRUD ------------------------------------------------

/// List groups with their current member counts. Archived groups are excluded
/// unless `include_archived` (the management view passes `true` to offer
/// restore). Ordered by `display_order` then name for a stable picker.
pub fn list_groups(
    conn: &mut DbConnection,
    include_archived: bool,
) -> QueryResult<Vec<AssetGroupResponse>> {
    let mut q = asset_groups::table.into_boxed();
    if !include_archived {
        q = q.filter(asset_groups::archived_at.is_null());
    }
    let groups: Vec<AssetGroup> = q
        .order((asset_groups::display_order.asc(), asset_groups::name.asc()))
        .load(conn)?;

    // One query for every group's member count rather than per-row.
    let counts: Vec<(i32, i64)> = asset_group_assignments::table
        .group_by(asset_group_assignments::group_id)
        .select((asset_group_assignments::group_id, diesel::dsl::count_star()))
        .load(conn)?;
    let counts: HashMap<i32, i64> = counts.into_iter().collect();

    Ok(groups
        .into_iter()
        .map(|group| {
            let asset_count = counts.get(&group.id).copied().unwrap_or(0);
            AssetGroupResponse { group, asset_count }
        })
        .collect())
}

/// Load one group plus its member count, the shape the list returns. Lets the
/// CRUD handlers answer with the same `AssetGroupResponse` the list does.
pub fn group_response(conn: &mut DbConnection, id: i32) -> QueryResult<AssetGroupResponse> {
    let group: AssetGroup = asset_groups::table.find(id).first(conn)?;
    let asset_count: i64 = asset_group_assignments::table
        .filter(asset_group_assignments::group_id.eq(id))
        .count()
        .get_result(conn)?;
    Ok(AssetGroupResponse { group, asset_count })
}

// sync-audit-only: asset-group definitions are NOT a sync aggregate (workspace config, picker re-fetches). Asset↔group assignment IS sync-wired via `asset.groups_changed` in `set_groups_for_asset`
pub fn create_group(conn: &mut DbConnection, new_group: NewAssetGroup) -> QueryResult<AssetGroup> {
    diesel::insert_into(asset_groups::table)
        .values(&new_group)
        .get_result(conn)
}

// sync-audit-only: asset-group definitions are NOT a sync aggregate (workspace config, picker re-fetches). Asset↔group assignment IS sync-wired via `asset.groups_changed` in `set_groups_for_asset`
pub fn update_group(
    conn: &mut DbConnection,
    id: i32,
    update: AssetGroupUpdate,
) -> QueryResult<AssetGroup> {
    diesel::update(asset_groups::table.find(id))
        .set(&update)
        .get_result(conn)
}

// sync-audit-only: asset-group definitions are NOT a sync aggregate (workspace config, picker re-fetches). Asset↔group assignment IS sync-wired via `asset.groups_changed` in `set_groups_for_asset`
/// Soft-archive a group (sets `archived_at`). Assignments stay so historical
/// references keep their target; archived groups drop out of the picker, and
/// the unique-name index frees the name for reuse.
pub fn archive_group(conn: &mut DbConnection, id: i32) -> QueryResult<AssetGroup> {
    diesel::update(asset_groups::table.find(id))
        .set(asset_groups::archived_at.eq(diesel::dsl::now))
        .get_result(conn)
}

// sync-audit-only: asset-group definitions are NOT a sync aggregate (workspace config, picker re-fetches). Asset↔group assignment IS sync-wired via `asset.groups_changed` in `set_groups_for_asset`
/// Clear `archived_at`, returning a group to the picker. May fail the active-
/// name unique index if the name was reused while archived; the caller
/// surfaces that.
pub fn restore_group(conn: &mut DbConnection, id: i32) -> QueryResult<AssetGroup> {
    diesel::update(asset_groups::table.find(id))
        .set(asset_groups::archived_at.eq(None::<chrono::NaiveDateTime>))
        .get_result(conn)
}

// ---- Asset ↔ group assignment ----------------------------------

/// Full (non-archived) group rows an asset belongs to, ordered for stable
/// rendering. Feeds the asset-detail pill row.
pub fn groups_for_asset(conn: &mut DbConnection, asset_id: i32) -> QueryResult<Vec<AssetGroup>> {
    asset_group_assignments::table
        .filter(asset_group_assignments::asset_id.eq(asset_id))
        .inner_join(asset_groups::table.on(asset_groups::id.eq(asset_group_assignments::group_id)))
        .filter(asset_groups::archived_at.is_null())
        .select(asset_groups::all_columns)
        .order((asset_groups::display_order.asc(), asset_groups::name.asc()))
        .load(conn)
}

/// Batched map asset_id → compact group refs for list/detail enrichment (one
/// query per page). Archived groups are excluded; ordered for stable chips.
pub fn group_refs_for_assets(
    conn: &mut DbConnection,
    asset_ids: &[i32],
) -> QueryResult<HashMap<i32, Vec<AssetGroupRef>>> {
    let rows: Vec<(i32, i32, Uuid, String, Option<String>)> = asset_group_assignments::table
        .inner_join(asset_groups::table.on(asset_groups::id.eq(asset_group_assignments::group_id)))
        .filter(asset_group_assignments::asset_id.eq_any(asset_ids))
        .filter(asset_groups::archived_at.is_null())
        .order((asset_groups::display_order.asc(), asset_groups::name.asc()))
        .select((
            asset_group_assignments::asset_id,
            asset_groups::id,
            asset_groups::uuid,
            asset_groups::name,
            asset_groups::color,
        ))
        .load(conn)?;
    let mut out: HashMap<i32, Vec<AssetGroupRef>> = HashMap::new();
    for (asset_id, id, uuid, name, color) in rows {
        out.entry(asset_id).or_default().push(AssetGroupRef {
            id,
            uuid,
            name,
            color,
        });
    }
    Ok(out)
}

/// Replace an asset's group set atomically (assigned from the asset side).
/// Computes the diff against the current set, applies it, and emits one
/// `asset.groups_changed` so list / detail views refresh. Idempotent: sending
/// the set already attached is a no-op (no emit). Returns the resulting,
/// sorted group ids.
pub fn set_groups_for_asset(
    conn: &mut DbConnection,
    asset_id: i32,
    desired_group_ids: &[i32],
    actor_uuid: Option<Uuid>,
) -> QueryResult<Vec<i32>> {
    use std::collections::HashSet;

    conn.transaction::<Vec<i32>, diesel::result::Error, _>(|conn| {
        // Resolve the asset up front: a clear "no such asset" error rather
        // than a deferred FK failure, and the row is the emit subject.
        let _asset: Asset = assets::table.find(asset_id).first(conn)?;

        let current: HashSet<i32> = asset_group_assignments::table
            .filter(asset_group_assignments::asset_id.eq(asset_id))
            .select(asset_group_assignments::group_id)
            .load::<i32>(conn)?
            .into_iter()
            .collect();
        // Keep only ids that name a live group in this workspace (RLS scopes
        // the query). Drops archived / unknown / cross-workspace ids so the PUT
        // can't record a membership that the archived-excluding reads would
        // then hide.
        let desired: HashSet<i32> = asset_groups::table
            .filter(asset_groups::id.eq_any(desired_group_ids))
            .filter(asset_groups::archived_at.is_null())
            .select(asset_groups::id)
            .load::<i32>(conn)?
            .into_iter()
            .collect();

        let to_add: Vec<i32> = desired.difference(&current).copied().collect();
        let to_remove: Vec<i32> = current.difference(&desired).copied().collect();

        if to_add.is_empty() && to_remove.is_empty() {
            let mut sorted: Vec<i32> = current.into_iter().collect();
            sorted.sort_unstable();
            return Ok(sorted);
        }

        if !to_remove.is_empty() {
            diesel::delete(
                asset_group_assignments::table
                    .filter(asset_group_assignments::asset_id.eq(asset_id))
                    .filter(asset_group_assignments::group_id.eq_any(&to_remove)),
            )
            .execute(conn)?;
        }

        if !to_add.is_empty() {
            let new_rows: Vec<NewAssetGroupAssignment> = to_add
                .iter()
                .map(|&group_id| NewAssetGroupAssignment {
                    group_id,
                    asset_id,
                    added_by: actor_uuid,
                })
                .collect();
            // ON CONFLICT guards a race where the membership already exists.
            diesel::insert_into(asset_group_assignments::table)
                .values(&new_rows)
                .on_conflict((
                    asset_group_assignments::group_id,
                    asset_group_assignments::asset_id,
                ))
                .do_nothing()
                .execute(conn)?;
        }

        let mut sorted: Vec<i32> = desired.into_iter().collect();
        sorted.sort_unstable();
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Asset,
                aggregate_id: asset_id.to_string(),
                op: SyncOp::Update,
                event_type: "asset.groups_changed",
                data: json!({
                    "id": asset_id,
                    "group_ids": sorted,
                    "added": to_add,
                    "removed": to_remove,
                }),
                groups: sync_groups::workspace(),
                causation_id: None,
            },
        )?;

        Ok(sorted)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NewAsset, NewAssetGroup};
    use crate::test_helpers::setup_test_connection;

    fn make_group(conn: &mut DbConnection, name: &str) -> AssetGroup {
        create_group(
            conn,
            NewAssetGroup {
                name: name.to_string(),
                description: None,
                color: None,
                display_order: 0,
                created_by: None,
            },
        )
        .unwrap()
    }

    fn make_asset(conn: &mut DbConnection, name: &str) -> i32 {
        let asset: Asset = diesel::insert_into(assets::table)
            .values(NewAsset {
                name: name.to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: None,
                notes: None,
                primary_user_uuid: None,
                purchase_date: None,
                asset_tag: None,
                kind: "generic".to_string(),
                attributes: serde_json::json!({}),
                quantity: None,
                unit: None,
                external_sync_source: None,
                low_stock_threshold: None,
            })
            .get_result(conn)
            .unwrap();
        asset.id
    }

    #[test]
    fn set_groups_diffs_and_is_idempotent() {
        let mut conn = setup_test_connection();
        let g1 = make_group(&mut conn, "Loaner pool");
        let g2 = make_group(&mut conn, "Exec laptops");
        let asset = make_asset(&mut conn, "laptop-1");

        let set = set_groups_for_asset(&mut conn, asset, &[g1.id, g2.id], None).unwrap();
        assert_eq!(set, {
            let mut v = vec![g1.id, g2.id];
            v.sort_unstable();
            v
        });

        // Re-sending the same set is a no-op that still reports the set.
        let again = set_groups_for_asset(&mut conn, asset, &[g2.id, g1.id], None).unwrap();
        assert_eq!(again, set);

        // Dropping g2 removes only that membership.
        let narrowed = set_groups_for_asset(&mut conn, asset, &[g1.id], None).unwrap();
        assert_eq!(narrowed, vec![g1.id]);

        let groups = groups_for_asset(&mut conn, asset).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, g1.id);
    }

    #[test]
    fn list_counts_and_archive_exclusion() {
        let mut conn = setup_test_connection();
        let g1 = make_group(&mut conn, "Warehouse scanners");
        let a1 = make_asset(&mut conn, "scanner-1");
        let a2 = make_asset(&mut conn, "scanner-2");
        set_groups_for_asset(&mut conn, a1, &[g1.id], None).unwrap();
        set_groups_for_asset(&mut conn, a2, &[g1.id], None).unwrap();

        let listed = list_groups(&mut conn, false).unwrap();
        let row = listed.iter().find(|r| r.group.id == g1.id).unwrap();
        assert_eq!(row.asset_count, 2);

        // Archived groups drop out of the default list and the per-asset view,
        // but the assignment row survives.
        archive_group(&mut conn, g1.id).unwrap();
        assert!(list_groups(&mut conn, false)
            .unwrap()
            .iter()
            .all(|r| r.group.id != g1.id));
        assert!(list_groups(&mut conn, true)
            .unwrap()
            .iter()
            .any(|r| r.group.id == g1.id));
        assert!(groups_for_asset(&mut conn, a1).unwrap().is_empty());
    }

    #[test]
    fn assigning_archived_or_unknown_group_is_dropped() {
        let mut conn = setup_test_connection();
        let live = make_group(&mut conn, "Live");
        let stale = make_group(&mut conn, "Stale");
        let asset = make_asset(&mut conn, "laptop-9");
        archive_group(&mut conn, stale.id).unwrap();

        // Archived (`stale`) and unknown (`999999`) ids are silently dropped;
        // only the live group is recorded, so the read-back matches the write.
        let saved =
            set_groups_for_asset(&mut conn, asset, &[live.id, stale.id, 999_999], None).unwrap();
        assert_eq!(saved, vec![live.id]);
        let groups = groups_for_asset(&mut conn, asset).unwrap();
        assert_eq!(
            groups.iter().map(|g| g.id).collect::<Vec<_>>(),
            vec![live.id]
        );
    }
}
