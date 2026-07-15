//! Push device token store (notification push channel).
//!
//! A user may have several devices. The push channel loads a recipient's ACTIVE
//! tokens (revoked_at IS NULL) to send to; the registration endpoints upsert /
//! revoke; the sender's invalid-token reports prune dead devices.

use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::schema::user_push_devices::dsl as d;

// sync-audit-only: push device tokens are server-side push infrastructure; no
// sync client subscribes to a user's device list.
/// Register (or refresh) a device token for a user. Upserts on the token so a
/// reinstall / a token reassigned to another user lands on one row, and
/// re-registering un-revokes it.
pub fn register(
    conn: &mut DbConnection,
    user: Uuid,
    workspace: i32,
    platform: &str,
    token: &str,
    app_version: Option<&str>,
) -> QueryResult<()> {
    let now = Utc::now().naive_utc();
    diesel::insert_into(d::user_push_devices)
        .values((
            d::user_uuid.eq(user),
            d::workspace_id.eq(workspace),
            d::platform.eq(platform),
            d::token.eq(token),
            d::app_version.eq(app_version),
            d::created_at.eq(now),
            d::updated_at.eq(now),
            d::last_seen_at.eq(now),
        ))
        .on_conflict(d::token)
        .do_update()
        .set((
            d::user_uuid.eq(user),
            d::workspace_id.eq(workspace),
            d::platform.eq(platform),
            d::app_version.eq(app_version),
            d::last_seen_at.eq(now),
            d::updated_at.eq(now),
            d::revoked_at.eq(None::<chrono::NaiveDateTime>),
        ))
        .execute(conn)?;
    Ok(())
}

// sync-audit-only: device-token lifecycle; no sync client subscribes to it.
/// Revoke a token for a user (logout / unregister). Rows affected.
pub fn revoke(conn: &mut DbConnection, user: Uuid, token: &str) -> QueryResult<usize> {
    let now = Utc::now().naive_utc();
    diesel::update(
        d::user_push_devices
            .filter(d::user_uuid.eq(user))
            .filter(d::token.eq(token)),
    )
    .set((d::revoked_at.eq(now), d::updated_at.eq(now)))
    .execute(conn)
}

/// A user's active `(platform, token)` pairs — the push channel's send list.
pub fn active_tokens_for_user(
    conn: &mut DbConnection,
    user: Uuid,
) -> QueryResult<Vec<(String, String)>> {
    d::user_push_devices
        .filter(d::user_uuid.eq(user))
        .filter(d::revoked_at.is_null())
        .select((d::platform, d::token))
        .load(conn)
}

// sync-audit-only: device-token lifecycle; no sync client subscribes to it.
/// Revoke tokens the provider reported as permanently invalid (APNs 410 / FCM
/// UNREGISTERED), so we stop sending to dead devices.
pub fn revoke_tokens(conn: &mut DbConnection, tokens: &[String]) -> QueryResult<usize> {
    if tokens.is_empty() {
        return Ok(0);
    }
    let now = Utc::now().naive_utc();
    diesel::update(d::user_push_devices.filter(d::token.eq_any(tokens)))
        .set((d::revoked_at.eq(now), d::updated_at.eq(now)))
        .execute(conn)
}
