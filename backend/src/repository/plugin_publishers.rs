//! Plugin publisher + local signing key repository
//!
//! Backs the trust chain for plugin signature verification:
//! `plugin_trusted_publishers` holds third-party keys synced from the
//! nosdesk.com registry; `plugin_local_signing_key` holds the
//! instance-local keypair used for the CLI install path.

use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::{
    LocalSigningKey, NewLocalSigningKey, NewTrustedPublisher, PluginRegistryState,
    PluginRegistryStateUpdate, TrustedPublisher,
};
use crate::schema::{plugin_local_signing_key, plugin_registry_state, plugin_trusted_publishers};

pub fn list_active_publishers(
    conn: &mut DbConnection,
) -> Result<Vec<TrustedPublisher>, diesel::result::Error> {
    plugin_trusted_publishers::table
        .filter(plugin_trusted_publishers::revoked_at.is_null())
        .order(plugin_trusted_publishers::display_name.asc())
        .load::<TrustedPublisher>(conn)
}

pub fn list_all_publishers(
    conn: &mut DbConnection,
) -> Result<Vec<TrustedPublisher>, diesel::result::Error> {
    plugin_trusted_publishers::table
        .order(plugin_trusted_publishers::display_name.asc())
        .load::<TrustedPublisher>(conn)
}

pub fn find_publisher_by_pubkey(
    conn: &mut DbConnection,
    pubkey: &str,
) -> Result<Option<TrustedPublisher>, diesel::result::Error> {
    plugin_trusted_publishers::table
        .filter(plugin_trusted_publishers::pubkey.eq(pubkey))
        .first::<TrustedPublisher>(conn)
        .optional()
}

// sync-audit-only: Plugin local storage / activity log — covered by the audit_log trigger on plugin_data and plugin_collection_rows
/// Insert a new publisher, or update the existing row matching on
/// `pubkey`. Used by the registry sync job to reconcile the keylist.
pub fn upsert_publisher(
    conn: &mut DbConnection,
    record: NewTrustedPublisher,
) -> Result<TrustedPublisher, diesel::result::Error> {
    diesel::insert_into(plugin_trusted_publishers::table)
        .values(&record)
        .on_conflict(plugin_trusted_publishers::pubkey)
        .do_update()
        .set(&record)
        .get_result::<TrustedPublisher>(conn)
}

// sync-audit-only: Plugin local storage / activity log — covered by the audit_log trigger on plugin_data and plugin_collection_rows
/// Mark a publisher as revoked. Existing plugins signed by this key
/// stay installed but new installs against the key fail.
pub fn revoke_publisher(
    conn: &mut DbConnection,
    pubkey: &str,
    at: chrono::NaiveDateTime,
) -> Result<usize, diesel::result::Error> {
    diesel::update(
        plugin_trusted_publishers::table.filter(plugin_trusted_publishers::pubkey.eq(pubkey)),
    )
    .set(plugin_trusted_publishers::revoked_at.eq(Some(at)))
    .execute(conn)
}

/// Load the single-row local signing key if it exists.
pub fn get_local_signing_key(
    conn: &mut DbConnection,
) -> Result<Option<LocalSigningKey>, diesel::result::Error> {
    plugin_local_signing_key::table
        .filter(plugin_local_signing_key::id.eq(1))
        .first::<LocalSigningKey>(conn)
        .optional()
}

// sync-audit-only: Plugin local storage / activity log — covered by the audit_log trigger on plugin_data and plugin_collection_rows
/// Insert the local signing key. The caller is responsible for
/// checking absence first; the CHECK(id = 1) on the table guarantees
/// a second insert fails.
pub fn insert_local_signing_key(
    conn: &mut DbConnection,
    record: NewLocalSigningKey,
) -> Result<LocalSigningKey, diesel::result::Error> {
    diesel::insert_into(plugin_local_signing_key::table)
        .values(&record)
        .get_result::<LocalSigningKey>(conn)
}

/// Load the singleton registry state. The row is seeded at migration
/// time with version=0, so callers never see `Option`.
pub fn get_registry_state(
    conn: &mut DbConnection,
) -> Result<PluginRegistryState, diesel::result::Error> {
    plugin_registry_state::table
        .filter(plugin_registry_state::id.eq(1))
        .first::<PluginRegistryState>(conn)
}

// sync-audit-only: Plugin local storage / activity log — covered by the audit_log trigger on plugin_data and plugin_collection_rows
pub fn update_registry_state(
    conn: &mut DbConnection,
    mut update: PluginRegistryStateUpdate,
) -> Result<PluginRegistryState, diesel::result::Error> {
    update.updated_at = Some(chrono::Utc::now().naive_utc());
    diesel::update(plugin_registry_state::table.filter(plugin_registry_state::id.eq(1)))
        .set(&update)
        .get_result::<PluginRegistryState>(conn)
}
