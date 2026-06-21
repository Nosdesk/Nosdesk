//! Repository for inbound forwarding addresses.
//!
//! A forwarding address is the opaque-token router for the hosted inbound
//! path: the customer forwards their support mailbox to
//! `<token>@inbound.<domain>`, and the inbound webhook resolves the token
//! back to the owning workspace + channel before running the existing
//! channels parse pipeline.
//!
//! Resolving a token ([`find_active_by_token`]) is a pre-tenant, cross-
//! workspace lookup: the webhook has no workspace context until the token
//! resolves, so it runs this query on a system/background connection. The
//! token's unguessability is the access control; RLS on the table is the
//! defence-in-depth backstop for ordinary app-path reads. Address creation
//! ([`create_for_channel`]) runs in the admin's workspace context, so the
//! RLS GUC fills `workspace_id` and the policy's `WITH CHECK` applies.

use diesel::prelude::*;
use diesel::QueryResult;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{InboundAddress, NewInboundAddress, INBOUND_ADDRESS_STATUS_ACTIVE};

/// Generate an opaque, email-localpart-safe forwarding token: 128 bits of
/// randomness (UUID v4) rendered as 32 lowercase hex characters. Unguessable,
/// so the address is a capability rather than something derivable from the
/// workspace slug, and safe as an email localpart (no quoting required).
pub fn generate_token() -> String {
    Uuid::new_v4().simple().to_string()
}

// sync-pending-wire: forwarding-address lifecycle gains its own sync event when the email_forward channel UI lands (Stage 3). For now the address is created alongside its channel, whose channel.created/channel.configured event already signals the config change, and the token is a routing capability we keep off the sync stream.
/// Create a forwarding address for a channel, minting a fresh token.
pub fn create_for_channel(conn: &mut DbConnection, channel_id: i32) -> QueryResult<InboundAddress> {
    use crate::schema::inbound_addresses::dsl as ia;
    let row = NewInboundAddress {
        token: generate_token(),
        channel_id,
    };
    diesel::insert_into(ia::inbound_addresses)
        .values(&row)
        .get_result(conn)
}

/// Resolve a forwarding token to its `active` address row. This is the
/// routing lookup the inbound webhook runs; `retired` tokens deliberately
/// don't resolve. Returns `Ok(None)` for an unknown or retired token.
pub fn find_active_by_token(
    conn: &mut DbConnection,
    token_value: &str,
) -> QueryResult<Option<InboundAddress>> {
    use crate::schema::inbound_addresses::dsl as ia;
    ia::inbound_addresses
        .filter(ia::token.eq(token_value))
        .filter(ia::status.eq(INBOUND_ADDRESS_STATUS_ACTIVE))
        .first(conn)
        .optional()
}

/// All addresses owned by a channel, oldest first. Drives the admin channel
/// page (show the address(es) the customer should forward to).
pub fn list_for_channel(
    conn: &mut DbConnection,
    channel_id_value: i32,
) -> QueryResult<Vec<InboundAddress>> {
    use crate::schema::inbound_addresses::dsl as ia;
    ia::inbound_addresses
        .filter(ia::channel_id.eq(channel_id_value))
        .order(ia::id.asc())
        .load(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::INBOUND_ADDRESS_STATUS_RETIRED;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn generate_token_is_unguessable_localpart() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    #[test]
    fn create_then_resolve_round_trips() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_forward");

        let addr = create_for_channel(&mut conn, ch.id).unwrap();
        assert_eq!(addr.channel_id, ch.id);
        assert_eq!(addr.status, INBOUND_ADDRESS_STATUS_ACTIVE);

        let found = find_active_by_token(&mut conn, &addr.token).unwrap();
        assert_eq!(found.unwrap().id, addr.id);
    }

    #[test]
    fn unknown_token_resolves_to_none() {
        let mut conn = setup_test_connection();
        assert!(find_active_by_token(&mut conn, "nope-not-a-real-token")
            .unwrap()
            .is_none());
    }

    #[test]
    fn retired_token_does_not_resolve() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_forward");
        let addr = create_for_channel(&mut conn, ch.id).unwrap();

        use crate::schema::inbound_addresses::dsl as ia;
        diesel::update(ia::inbound_addresses.find(addr.id))
            .set(ia::status.eq(INBOUND_ADDRESS_STATUS_RETIRED))
            .execute(&mut conn)
            .unwrap();

        assert!(find_active_by_token(&mut conn, &addr.token)
            .unwrap()
            .is_none());
    }

    #[test]
    fn list_for_channel_returns_owned_addresses() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_forward");
        let a1 = create_for_channel(&mut conn, ch.id).unwrap();
        let a2 = create_for_channel(&mut conn, ch.id).unwrap();

        let listed = list_for_channel(&mut conn, ch.id).unwrap();
        let ids: Vec<i32> = listed.iter().map(|a| a.id).collect();
        assert!(ids.contains(&a1.id));
        assert!(ids.contains(&a2.id));
    }

    #[test]
    fn token_unique_across_channels() {
        // Two addresses never collide; the unique index on `token` plus the
        // 128-bit generator make a clash astronomically unlikely, but the
        // resolver also keys on it, so assert distinctness explicitly.
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_forward");
        let a1 = create_for_channel(&mut conn, ch.id).unwrap();
        let a2 = create_for_channel(&mut conn, ch.id).unwrap();
        assert_ne!(a1.token, a2.token);
    }
}
