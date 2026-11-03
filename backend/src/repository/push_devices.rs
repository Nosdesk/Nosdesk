//! Push device token store (notification push channel).
//!
//! A user may have several devices. The push channel loads a recipient's ACTIVE
//! tokens (revoked_at IS NULL) to send to; the registration endpoints upsert /
//! revoke; the sender's invalid-token reports prune dead devices.
//!
//! Each registration belongs to the session that made it and is deleted with
//! it (`session_id`, ON DELETE CASCADE), so however a session ends, its device
//! stops receiving the person's notifications.

use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::schema::user_push_devices::dsl as d;

// sync-audit-only: server-side push infra; no sync client subscribes to a device list.
/// Register (or refresh) a device token for a user. Upserts on the token so a
/// reinstall / a token reassigned to another user lands on one row, and
/// re-registering un-revokes it. The row moves to `session`, so a sign-in on
/// the same phone carries the token over to the new session.
pub fn register(
    conn: &mut DbConnection,
    user: Uuid,
    session: Uuid,
    workspace: i32,
    platform: &str,
    token: &str,
    app_version: Option<&str>,
) -> QueryResult<()> {
    let now = Utc::now().naive_utc();
    diesel::insert_into(d::user_push_devices)
        .values((
            d::user_uuid.eq(user),
            d::session_id.eq(session),
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
            d::session_id.eq(session),
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

/// Whether the user has at least one live device.
///
/// The push channel's default hangs off this: with nothing registered there is
/// nothing to send to, so push resolves to `off` rather than offering a channel
/// that cannot deliver. Devices span the user's workspaces, so this is keyed by
/// user alone.
pub fn has_active_device(conn: &mut DbConnection, user: Uuid) -> QueryResult<bool> {
    use diesel::dsl::exists;
    diesel::select(exists(
        d::user_push_devices
            .filter(d::user_uuid.eq(user))
            .filter(d::revoked_at.is_null()),
    ))
    .get_result(conn)
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
