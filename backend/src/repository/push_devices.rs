//! Push device token store (notification push channel).
//!
//! A user may have several devices. The push channel loads a recipient's ACTIVE
//! tokens (revoked_at IS NULL) to send to; the registration endpoints upsert /
//! revoke; the sender's invalid-token reports prune dead devices.

use chrono::Utc;
use diesel::prelude::*;
use diesel::sql_types::{Array, Integer, Text, Uuid as SqlUuid};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::schema::user_push_devices::dsl as d;

// sync-audit-only: server-side push infra; no sync client subscribes to a device list.
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

/// Notification types that get push enabled the first time a user registers a
/// device.
///
/// Both are addressed to one person by name: an assignment makes a ticket
/// yours, a mention asks for you specifically. Comment activity is deliberately
/// absent — on a busy ticket it is the noisiest thing a helpdesk produces, and a
/// phone buzzing for every reply is how users learn to disable push entirely.
const PUSH_ON_FIRST_DEVICE: &[&str] = &["ticket_assigned", "mentioned"];

// sync-audit-only: preference seeding; no sync client subscribes to preferences.
/// Give a user sensible push preferences the first time one of their devices
/// registers.
///
/// Push is off by default for every type, which is the right default for a
/// browser: there is nothing to send to and no OS permission. But the mobile app
/// asks for notification permission at sign-in, and granting it registered a
/// device and then changed nothing observable — the prompt promised something
/// the product did not deliver. This closes that gap at the only moment where
/// consent and capability both exist.
///
/// `DO NOTHING` per row, so this can never overwrite a choice. A user who turns
/// mentions off and later signs in on a second device keeps them off; the seed
/// simply finds a row and declines. That also makes it idempotent, matching
/// [`register`], which the client may call on every launch.
pub fn seed_push_defaults(
    conn: &mut DbConnection,
    user: Uuid,
    workspace: i32,
) -> QueryResult<usize> {
    diesel::sql_query(
        "INSERT INTO notification_preferences \
           (user_uuid, notification_type_id, channel, enabled, frequency, \
            workspace_id, created_at, updated_at) \
         SELECT $1, nt.id, 'push', TRUE, 'instant', $2, now(), now() \
         FROM notification_types nt \
         WHERE nt.code = ANY($3) \
         ON CONFLICT (user_uuid, notification_type_id, channel) DO NOTHING",
    )
    .bind::<SqlUuid, _>(user)
    .bind::<Integer, _>(workspace)
    .bind::<Array<Text>, _>(
        PUSH_ON_FIRST_DEVICE
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>(),
    )
    .execute(conn)
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
