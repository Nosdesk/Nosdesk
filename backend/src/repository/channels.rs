//! Repository functions for the channels framework.
//!
//! Three concerns live here:
//!   1. CRUD on `channels` rows (admin config).
//!   2. Encrypted secrets via `channel_credentials` — plaintext never
//!      leaves this module once stored.
//!   3. Dedup + threading lookups on `channel_messages`.
//!
//! Higher-level orchestration (which adapter handles which provider,
//! thread-resolution cascade) lives in `services::channels`.

use diesel::prelude::*;
use diesel::QueryResult;

use crate::db::DbConnection;
use crate::models::{
    Channel, ChannelCredential, ChannelMessage, ChannelUpdate, NewChannel, NewChannelCredential,
    NewChannelMessage,
};
use crate::utils::encryption;

// ---------- channels table ----------

pub fn list_channels(conn: &mut DbConnection) -> QueryResult<Vec<Channel>> {
    use crate::schema::channels::dsl::*;
    channels.order(id.asc()).load(conn)
}

/// Only channels the registry should actively run. Used at startup.
pub fn list_enabled(conn: &mut DbConnection) -> QueryResult<Vec<Channel>> {
    use crate::schema::channels::dsl::*;
    channels.filter(enabled.eq(true)).order(id.asc()).load(conn)
}

pub fn find(conn: &mut DbConnection, channel_id: i32) -> QueryResult<Channel> {
    use crate::schema::channels::dsl::*;
    channels.find(channel_id).first(conn)
}

/// Find-or-create by `provider` name. Used by the phase-1 single-mailbox
/// admin UI which upserts the one `email_imap` row; multi-mailbox UIs can
/// call `create`/`update` directly keyed on `id`.
pub fn find_by_provider(
    conn: &mut DbConnection,
    provider_name: &str,
) -> QueryResult<Option<Channel>> {
    use crate::schema::channels::dsl::*;
    channels
        .filter(provider.eq(provider_name))
        .first(conn)
        .optional()
}

pub fn create(conn: &mut DbConnection, new: NewChannel) -> QueryResult<Channel> {
    use crate::schema::channels::dsl::*;
    diesel::insert_into(channels).values(&new).get_result(conn)
}

pub fn update(
    conn: &mut DbConnection,
    channel_id: i32,
    change: ChannelUpdate,
) -> QueryResult<Channel> {
    use crate::schema::channels::dsl::*;
    diesel::update(channels.find(channel_id))
        .set(&change)
        .get_result(conn)
}

pub fn delete(conn: &mut DbConnection, channel_id: i32) -> QueryResult<usize> {
    use crate::schema::channels::dsl::*;
    diesel::delete(channels.find(channel_id)).execute(conn)
}

/// Persist the adapter's runtime state (e.g. last IMAP UID). Narrower than
/// `update()` so adapters don't accidentally touch the user-editable
/// config on every poll tick.
pub fn update_runtime_state(
    conn: &mut DbConnection,
    channel_id: i32,
    state: serde_json::Value,
) -> QueryResult<usize> {
    use crate::schema::channels::dsl::*;
    diesel::update(channels.find(channel_id))
        .set((
            runtime_state.eq(state),
            last_polled_at.eq(chrono::Utc::now().naive_utc()),
        ))
        .execute(conn)
}

// ---------- channel_credentials table ----------

/// Error returned when encryption / decryption fails. We wrap Diesel and
/// encryption errors in a single type so callers don't have to stitch
/// them together at every call site.
#[derive(Debug)]
pub enum CredentialError {
    Db(diesel::result::Error),
    Crypto(String),
}

impl std::fmt::Display for CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Db(e) => write!(f, "database error: {e}"),
            Self::Crypto(m) => write!(f, "credential encryption error: {m}"),
        }
    }
}
impl std::error::Error for CredentialError {}

impl From<diesel::result::Error> for CredentialError {
    fn from(e: diesel::result::Error) -> Self {
        Self::Db(e)
    }
}

/// Store (or replace) an encrypted credential for a channel. Upserts on
/// the `(channel_id, credential_type)` unique index so rotating a password
/// is a single call.
pub fn put_credential(
    conn: &mut DbConnection,
    channel_id: i32,
    credential_type: &str,
    plaintext: &str,
    expires_at: Option<chrono::NaiveDateTime>,
) -> Result<(), CredentialError> {
    use crate::schema::channel_credentials::dsl as cc;

    let encrypted =
        encryption::encrypt(plaintext).map_err(|e| CredentialError::Crypto(e.to_string()))?;

    let row = NewChannelCredential {
        channel_id,
        credential_type: credential_type.to_string(),
        encrypted_value: encrypted,
        expires_at,
    };

    diesel::insert_into(cc::channel_credentials)
        .values(&row)
        .on_conflict((cc::channel_id, cc::credential_type))
        .do_update()
        .set((
            cc::encrypted_value.eq(diesel::upsert::excluded(cc::encrypted_value)),
            cc::expires_at.eq(diesel::upsert::excluded(cc::expires_at)),
        ))
        .execute(conn)?;

    Ok(())
}

/// Fetch and decrypt a credential. Returns `Ok(None)` if no row exists
/// for that `(channel_id, credential_type)`, `Err` only on DB failure or
/// a decryption error (which should mean the `ENCRYPTION_KEY` changed
/// and stored secrets are now unreadable).
pub fn get_credential(
    conn: &mut DbConnection,
    channel_id: i32,
    credential_type: &str,
) -> Result<Option<String>, CredentialError> {
    use crate::schema::channel_credentials::dsl as cc;

    let row: Option<ChannelCredential> = cc::channel_credentials
        .filter(cc::channel_id.eq(channel_id))
        .filter(cc::credential_type.eq(credential_type))
        .first(conn)
        .optional()?;

    match row {
        Some(r) => {
            let plaintext = encryption::decrypt(&r.encrypted_value)
                .map_err(|e| CredentialError::Crypto(e.to_string()))?;
            Ok(Some(plaintext))
        }
        None => Ok(None),
    }
}

pub fn delete_credential(
    conn: &mut DbConnection,
    channel_id: i32,
    credential_type: &str,
) -> QueryResult<usize> {
    use crate::schema::channel_credentials::dsl as cc;
    diesel::delete(
        cc::channel_credentials
            .filter(cc::channel_id.eq(channel_id))
            .filter(cc::credential_type.eq(credential_type)),
    )
    .execute(conn)
}

// ---------- channel_messages table ----------

/// Record a message we processed (inbound or outbound). Idempotent on the
/// `(channel_id, external_id, direction)` unique index — if we somehow
/// process the same message twice we get the existing row back.
pub fn record_message(
    conn: &mut DbConnection,
    new: NewChannelMessage,
) -> QueryResult<ChannelMessage> {
    use crate::schema::channel_messages::dsl as cm;
    diesel::insert_into(cm::channel_messages)
        .values(&new)
        .on_conflict((cm::channel_id, cm::external_id, cm::direction))
        .do_update()
        .set(cm::received_at.eq(diesel::upsert::excluded(cm::received_at)))
        .get_result(conn)
}

/// Look up a single `channel_messages` row by its channel-scoped
/// external id. Used by the thread resolver to walk `In-Reply-To` /
/// `References` chains back to the ticket.
pub fn find_by_external_id(
    conn: &mut DbConnection,
    channel_id: i32,
    external_id_value: &str,
) -> QueryResult<Option<ChannelMessage>> {
    use crate::schema::channel_messages::dsl as cm;
    cm::channel_messages
        .filter(cm::channel_id.eq(channel_id))
        .filter(cm::external_id.eq(external_id_value))
        .first(conn)
        .optional()
}

/// Look up the channel-recorded `from_address` for each of the given
/// comment ids in a single query. Returned as a `HashMap` keyed by
/// `comment_id` so callers can do a cheap O(1) merge with the rest of
/// the comment payload.
///
/// Used by the comment list endpoint to surface the customer's email
/// (or future chat sender's address) on the second line of the comment
/// header — even for comments older than when the inbound pipeline
/// started stamping `from_address` into `channel_metadata` directly.
pub fn from_addresses_for_comments(
    conn: &mut DbConnection,
    comment_ids: &[i32],
) -> QueryResult<std::collections::HashMap<i32, String>> {
    use crate::schema::channel_messages::dsl as cm;
    if comment_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows: Vec<(Option<i32>, Option<String>)> = cm::channel_messages
        .filter(cm::comment_id.eq_any(comment_ids))
        .filter(cm::from_address.is_not_null())
        .select((cm::comment_id, cm::from_address))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .filter_map(|(cid, addr)| match (cid, addr) {
            (Some(c), Some(a)) => Some((c, a)),
            _ => None,
        })
        .collect())
}

/// Walk a list of parent external IDs (In-Reply-To + References chain)
/// and return the first ticket id we find. Batched into a single SQL
/// query — this runs on every inbound message so it needs to be cheap.
pub fn find_ticket_by_reference_chain(
    conn: &mut DbConnection,
    channel_id: i32,
    external_ids: &[String],
) -> QueryResult<Option<i32>> {
    use crate::schema::channel_messages::dsl as cm;
    if external_ids.is_empty() {
        return Ok(None);
    }
    let row: Option<Option<i32>> = cm::channel_messages
        .filter(cm::channel_id.eq(channel_id))
        .filter(cm::external_id.eq_any(external_ids))
        .filter(cm::ticket_id.is_not_null())
        .select(cm::ticket_id)
        .first(conn)
        .optional()?;
    // `select(ticket_id)` yields Option<i32>; `.optional()` wraps it again.
    Ok(row.flatten())
}

/// Record the last-seen message in a thread so adapters that need a
/// parent reference (e.g. email In-Reply-To) can look it up when
/// sending a tech's reply.
pub fn latest_inbound_for_ticket(
    conn: &mut DbConnection,
    channel_id: i32,
    ticket_id: i32,
) -> QueryResult<Option<ChannelMessage>> {
    use crate::models::CHANNEL_DIRECTION_INBOUND;
    use crate::schema::channel_messages::dsl as cm;
    cm::channel_messages
        .filter(cm::channel_id.eq(channel_id))
        .filter(cm::ticket_id.eq(ticket_id))
        .filter(cm::direction.eq(CHANNEL_DIRECTION_INBOUND))
        .order(cm::received_at.desc())
        .first(conn)
        .optional()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{UserRole, CHANNEL_DIRECTION_INBOUND, CHANNEL_DIRECTION_OUTBOUND};
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use serde_json::json;

    #[test]
    fn channel_crud_roundtrip() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        assert_eq!(ch.provider, "email_imap");
        assert!(ch.enabled);

        let fetched = find(&mut conn, ch.id).unwrap();
        assert_eq!(fetched.id, ch.id);

        let listed = list_enabled(&mut conn).unwrap();
        assert!(listed.iter().any(|c| c.id == ch.id));

        let updated = update(
            &mut conn,
            ch.id,
            ChannelUpdate {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!updated.enabled);

        let enabled_only = list_enabled(&mut conn).unwrap();
        assert!(!enabled_only.iter().any(|c| c.id == ch.id));
    }

    #[test]
    fn find_by_provider_returns_singleton() {
        // Unique provider string so this test is independent of any
        // rows the dev DB already has (a real `email_imap` mailbox is
        // common during local development).
        let provider = format!("test-unique-{}", uuid::Uuid::new_v4());
        let mut conn = setup_test_connection();
        assert!(find_by_provider(&mut conn, &provider).unwrap().is_none());
        let ch = TestFixtures::create_channel(&mut conn, &provider);
        let found = find_by_provider(&mut conn, &provider).unwrap();
        assert_eq!(found.unwrap().id, ch.id);
    }

    #[test]
    fn runtime_state_update_persists() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        update_runtime_state(&mut conn, ch.id, json!({ "last_seen_uid": 42 })).unwrap();
        let fetched = find(&mut conn, ch.id).unwrap();
        assert_eq!(fetched.runtime_state["last_seen_uid"], 42);
        assert!(fetched.last_polled_at.is_some());
    }

    #[test]
    fn credential_roundtrip_encrypts_at_rest() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");

        put_credential(&mut conn, ch.id, "imap_password", "hunter2", None).unwrap();

        // Raw row has ciphertext, not plaintext.
        use crate::schema::channel_credentials::dsl as cc;
        let raw: ChannelCredential = cc::channel_credentials
            .filter(cc::channel_id.eq(ch.id))
            .filter(cc::credential_type.eq("imap_password"))
            .first(&mut conn)
            .unwrap();
        assert_ne!(raw.encrypted_value, "hunter2");

        // Decrypted value round-trips.
        let decrypted = get_credential(&mut conn, ch.id, "imap_password").unwrap();
        assert_eq!(decrypted.as_deref(), Some("hunter2"));
    }

    #[test]
    fn credential_upsert_replaces_existing() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");

        put_credential(&mut conn, ch.id, "imap_password", "old", None).unwrap();
        put_credential(&mut conn, ch.id, "imap_password", "new", None).unwrap();

        assert_eq!(
            get_credential(&mut conn, ch.id, "imap_password")
                .unwrap()
                .as_deref(),
            Some("new")
        );
    }

    #[test]
    fn credential_missing_returns_none() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        assert!(get_credential(&mut conn, ch.id, "imap_password")
            .unwrap()
            .is_none());
    }

    #[test]
    fn record_message_is_idempotent_on_same_external_id() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");

        let msg1 = record_message(
            &mut conn,
            NewChannelMessage {
                channel_id: ch.id,
                external_id: "<abc@example.com>".to_string(),
                direction: CHANNEL_DIRECTION_INBOUND.to_string(),
                ticket_id: None,
                comment_id: None,
                in_reply_to: None,
                from_address: Some("alice@example.com".to_string()),
                author_user_uuid: None,
                raw_metadata: None,
            },
        )
        .unwrap();

        // Replaying the same external_id returns the same row.
        let msg2 = record_message(
            &mut conn,
            NewChannelMessage {
                channel_id: ch.id,
                external_id: "<abc@example.com>".to_string(),
                direction: CHANNEL_DIRECTION_INBOUND.to_string(),
                ticket_id: None,
                comment_id: None,
                in_reply_to: None,
                from_address: Some("alice@example.com".to_string()),
                author_user_uuid: None,
                raw_metadata: None,
            },
        )
        .unwrap();

        assert_eq!(msg1.id, msg2.id);
    }

    #[test]
    fn find_ticket_by_reference_chain_matches_first_hit() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "u", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");

        record_message(
            &mut conn,
            NewChannelMessage {
                channel_id: ch.id,
                external_id: "<parent@x>".to_string(),
                direction: CHANNEL_DIRECTION_OUTBOUND.to_string(),
                ticket_id: Some(ticket.id),
                comment_id: None,
                in_reply_to: None,
                from_address: None,
                author_user_uuid: None,
                raw_metadata: None,
            },
        )
        .unwrap();

        // Chain includes a miss and a hit; we should find the ticket via the hit.
        let references = vec!["<unknown@somewhere>".to_string(), "<parent@x>".to_string()];
        let hit = find_ticket_by_reference_chain(&mut conn, ch.id, &references).unwrap();
        assert_eq!(hit, Some(ticket.id));

        let miss =
            find_ticket_by_reference_chain(&mut conn, ch.id, &["<nope@nope>".to_string()]).unwrap();
        assert_eq!(miss, None);
    }

    #[test]
    fn find_ticket_by_reference_chain_empty_is_none() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        assert_eq!(
            find_ticket_by_reference_chain(&mut conn, ch.id, &[]).unwrap(),
            None
        );
    }

    #[test]
    fn cascade_delete_on_channel_removes_credentials_and_messages() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        put_credential(&mut conn, ch.id, "imap_password", "x", None).unwrap();
        record_message(
            &mut conn,
            NewChannelMessage {
                channel_id: ch.id,
                external_id: "<m@x>".to_string(),
                direction: CHANNEL_DIRECTION_INBOUND.to_string(),
                ticket_id: None,
                comment_id: None,
                in_reply_to: None,
                from_address: None,
                author_user_uuid: None,
                raw_metadata: None,
            },
        )
        .unwrap();

        assert_eq!(delete(&mut conn, ch.id).unwrap(), 1);

        // Credentials and messages gone too (FK CASCADE).
        use crate::schema::channel_credentials::dsl as cc;
        use crate::schema::channel_messages::dsl as cm;
        let creds_left: i64 = cc::channel_credentials
            .filter(cc::channel_id.eq(ch.id))
            .count()
            .get_result(&mut conn)
            .unwrap();
        let msgs_left: i64 = cm::channel_messages
            .filter(cm::channel_id.eq(ch.id))
            .count()
            .get_result(&mut conn)
            .unwrap();
        assert_eq!(creds_left, 0);
        assert_eq!(msgs_left, 0);
    }
}
