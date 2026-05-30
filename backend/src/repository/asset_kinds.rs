//! Asset-kinds repository.
//!
//! The `asset_kinds` table is the runtime registry that drives
//! the `assets.kind` discriminator. Built-ins ship with the
//! product (slug + label + is_builtin = true); admins can add
//! their own kinds at runtime with a constrained JSON Schema
//! attribute spec validated by `services::assets::kinds`.
//!
//! Kinds are NOT a sync aggregate today. The admin picker
//! re-fetches on demand and asset rows store the slug as a
//! plain string, so the registry can be reloaded lazily without
//! a fanout. If asset-kind chips ever need real-time updates we
//! can wire them in then.

use chrono::Utc;
use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::{AssetKind, AssetKindUpdate, NewAssetKind};
use crate::schema::asset_kinds;

pub fn list_kinds(conn: &mut DbConnection) -> QueryResult<Vec<AssetKind>> {
    asset_kinds::table
        .order((asset_kinds::sort_order.asc(), asset_kinds::label.asc()))
        .load(conn)
}

pub fn get_kind(conn: &mut DbConnection, id: i32) -> QueryResult<AssetKind> {
    asset_kinds::table.find(id).first(conn)
}

pub fn get_kind_by_slug(conn: &mut DbConnection, slug: &str) -> QueryResult<AssetKind> {
    asset_kinds::table
        .filter(asset_kinds::slug.eq(slug))
        .first(conn)
}

// sync-audit-only: kind registry is workspace config, not a sync aggregate. Admin pickers re-fetch on demand; coverage lives in the audit_log trigger attached in 2026-05-20-110000_attach_audit_assets
pub fn create_kind(conn: &mut DbConnection, new_kind: NewAssetKind) -> QueryResult<AssetKind> {
    diesel::insert_into(asset_kinds::table)
        .values(&new_kind)
        .get_result(conn)
}

// sync-audit-only: kind registry is workspace config, see create_kind
pub fn update_kind(
    conn: &mut DbConnection,
    id: i32,
    mut update: AssetKindUpdate,
) -> QueryResult<AssetKind> {
    update.updated_at = Some(Utc::now());
    diesel::update(asset_kinds::table.find(id))
        .set(&update)
        .get_result(conn)
}

// sync-audit-only: kind registry is workspace config, see create_kind
/// Delete an asset kind. The caller is responsible for refusing
/// builtin slugs first (see `handlers::asset_kinds::delete`),
/// which gives the admin UI a distinct "can't delete a builtin"
/// error instead of the generic "not found" you'd get from a
/// post-filter here.
pub fn delete_kind(conn: &mut DbConnection, id: i32) -> QueryResult<usize> {
    diesel::delete(asset_kinds::table.find(id)).execute(conn)
}

/// Count how many asset rows currently carry `slug` as their kind
/// discriminator. Drives the delete-usage guard on the admin page
/// (the ConfirmModal shows "N assets currently use this kind"
/// rather than silently orphaning rows) and the per-kind usage
/// stat in the list view. RLS is already pinned by the calling
/// TenantConn so the count is workspace-local automatically.
pub fn count_assets_using_kind(conn: &mut DbConnection, slug: &str) -> QueryResult<i64> {
    use crate::schema::assets;
    assets::table
        .filter(assets::kind.eq(slug))
        .count()
        .get_result(conn)
}
