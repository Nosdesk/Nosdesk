//! What a collab session may say in the room's awareness.
//!
//! Awareness is the room's ephemeral state: each client's cursor, its user
//! (name, colour, avatar, uuid) and, for tickets, who is on the ticket and
//! what they are typing. Peers trust it to say who someone is, so the server
//! attests it rather than relaying what a client claims:
//!
//! - A session speaks only for its own Yjs client id, pinned from its first
//!   awareness frame. Entries for any other client are dropped. The stock
//!   y-websocket client echoes the changes it hears from others; the server
//!   already holds those, so dropping the echo loses nothing.
//! - A state's `user.uuid` is the session's signed-in user, whatever the
//!   client wrote there.
//! - A state is a JSON object of bounded size, or `null` (the client left).

use std::sync::Arc;

use uuid::Uuid;
use yrs::encoding::read::Cursor;
use yrs::sync::{AwarenessUpdate, Message, MessageReader};
use yrs::updates::decoder::DecoderV1;
use yrs::updates::encoder::Encode;
use yrs::ClientID;

/// The largest awareness state a session may publish, in bytes of JSON. Room
/// for a cursor and the longest field preview (ticket resolution notes,
/// 50,000 bytes) with headroom.
pub const MAX_STATE_BYTES: usize = 64 * 1024;

/// An inbound frame after attestation.
#[derive(Debug, PartialEq, Eq)]
pub enum Attested {
    /// No awareness in it: relay and apply the frame as it came.
    Unchanged,
    /// Awareness in it was narrowed or stamped: use these bytes instead.
    Rewritten(Vec<u8>),
    /// Nothing is left to apply or relay.
    Empty,
    /// The frame doesn't decode as y-protocol messages.
    Malformed,
}

/// Attest the awareness in `frame`, one inbound WebSocket frame of y-protocol
/// messages, for a session whose pinned client id is `own` (`None` before it
/// has one) and whose signed-in user is `user`.
pub fn attest(frame: &[u8], own: Option<u64>, user: Uuid) -> Attested {
    let mut decoder = DecoderV1::new(Cursor::new(frame));
    let mut messages = Vec::new();
    for message in MessageReader::new(&mut decoder) {
        match message {
            Ok(message) => messages.push(message),
            Err(_) => return Attested::Malformed,
        }
    }
    if !messages.iter().any(|m| matches!(m, Message::Awareness(_))) {
        return Attested::Unchanged;
    }

    let mut kept = Vec::with_capacity(messages.len());
    for message in messages {
        match message {
            Message::Awareness(update) => {
                if let Some(update) = own_entry(update, own, user) {
                    kept.push(Message::Awareness(update));
                }
            }
            other => kept.push(other),
        }
    }
    if kept.is_empty() {
        return Attested::Empty;
    }
    Attested::Rewritten(kept.iter().flat_map(Encode::encode_v1).collect())
}

/// The client id a session's first awareness frame names, when it names
/// exactly one: what the session is pinned to. An echo carrying several
/// clients pins nothing.
pub fn sole_client(frame: &[u8]) -> Option<u64> {
    let mut decoder = DecoderV1::new(Cursor::new(frame));
    let mut found = None;
    for message in MessageReader::new(&mut decoder) {
        if let Ok(Message::Awareness(update)) = message {
            let mut clients = update.clients.keys();
            match (clients.next(), clients.next(), found) {
                (Some(client), None, None) => found = Some(client.get()),
                (Some(client), None, Some(seen)) if seen == client.get() => {}
                _ => return None,
            }
        }
    }
    found
}

/// `update` narrowed to the session's own entry, attested; `None` when
/// nothing of it may stand.
fn own_entry(mut update: AwarenessUpdate, own: Option<u64>, user: Uuid) -> Option<AwarenessUpdate> {
    let own = ClientID::new(own?);
    let mut entry = update.clients.remove(&own)?;
    entry.json = attest_state(&entry.json, user)?;
    update.clients.clear();
    update.clients.insert(own, entry);
    Some(update)
}

/// A state's JSON with `user.uuid` set to `user`, or `None` when it is too
/// large or not a JSON object. `null` (the client left) stands as it is.
fn attest_state(json: &str, user: Uuid) -> Option<Arc<str>> {
    if json.len() > MAX_STATE_BYTES {
        return None;
    }
    let mut state: serde_json::Value = serde_json::from_str(json).ok()?;
    match &mut state {
        serde_json::Value::Null => return Some(Arc::from("null")),
        serde_json::Value::Object(fields) => match fields.get_mut("user") {
            None => {}
            Some(serde_json::Value::Object(who)) => {
                who.insert("uuid".into(), serde_json::Value::String(user.to_string()));
            }
            Some(_) => return None,
        },
        _ => return None,
    }
    Some(Arc::from(state.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use yrs::sync::awareness::AwarenessUpdateEntry;
    use yrs::sync::SyncMessage;

    const OWN: u64 = 1234;
    const OTHER: u64 = 5678;

    fn update(entries: &[(u64, &str)]) -> Message {
        let clients: HashMap<ClientID, AwarenessUpdateEntry> = entries
            .iter()
            .map(|(id, json)| {
                (
                    ClientID::new(*id),
                    AwarenessUpdateEntry {
                        clock: 1,
                        json: Arc::from(*json),
                    },
                )
            })
            .collect();
        Message::Awareness(AwarenessUpdate { clients })
    }

    fn frame(messages: &[Message]) -> Vec<u8> {
        messages.iter().flat_map(Encode::encode_v1).collect()
    }

    /// The awareness entries of an attested frame, by client id.
    fn entries(attested: Attested) -> Vec<(u64, serde_json::Value)> {
        let Attested::Rewritten(bytes) = attested else {
            panic!("expected a rewritten frame, got {attested:?}");
        };
        let mut decoder = DecoderV1::new(Cursor::new(&bytes));
        let mut out = Vec::new();
        for message in MessageReader::new(&mut decoder) {
            if let Ok(Message::Awareness(update)) = message {
                for (id, entry) in update.clients {
                    out.push((id.get(), serde_json::from_str(&entry.json).expect("json")));
                }
            }
        }
        out
    }

    #[test]
    fn a_claimed_identity_is_replaced_by_the_sessions_user() {
        let me = Uuid::new_v4();
        let f = frame(&[update(&[(
            OWN,
            r#"{"user":{"name":"Sam","uuid":"00000000-0000-0000-0000-000000000001"},"cursor":null}"#,
        )])]);
        let got = entries(attest(&f, Some(OWN), me));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, OWN);
        assert_eq!(got[0].1["user"]["uuid"], me.to_string());
        assert_eq!(got[0].1["user"]["name"], "Sam");
        assert!(got[0].1.get("cursor").is_some(), "other fields stand");
    }

    #[test]
    fn only_the_sessions_own_client_stands() {
        let me = Uuid::new_v4();
        let f = frame(&[update(&[
            (OWN, r#"{"user":{"name":"Me"}}"#),
            (OTHER, r#"{"user":{"name":"Someone else"}}"#),
        ])]);
        let got = entries(attest(&f, Some(OWN), me));
        assert_eq!(got.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![OWN]);

        // An echo of someone else's state alone leaves nothing to relay.
        let echo = frame(&[update(&[(OTHER, r#"{"user":{"name":"Someone else"}}"#)])]);
        assert_eq!(attest(&echo, Some(OWN), me), Attested::Empty);
        // Before a session is pinned, it speaks for no one.
        assert_eq!(attest(&echo, None, me), Attested::Empty);
    }

    #[test]
    fn awareness_hidden_behind_a_sync_message_is_attested_too() {
        let me = Uuid::new_v4();
        let f = frame(&[
            Message::Sync(SyncMessage::Update(vec![0, 0])),
            update(&[(OTHER, r#"{"user":{"name":"Someone else"}}"#)]),
        ]);
        let Attested::Rewritten(bytes) = attest(&f, Some(OWN), me) else {
            panic!("expected a rewrite");
        };
        assert_eq!(
            bytes,
            frame(&[Message::Sync(SyncMessage::Update(vec![0, 0]))])
        );
    }

    #[test]
    fn a_frame_without_awareness_is_untouched() {
        let f = frame(&[Message::Sync(SyncMessage::Update(vec![0, 0]))]);
        assert_eq!(attest(&f, Some(OWN), Uuid::new_v4()), Attested::Unchanged);
        // A sync message of a kind y-protocols doesn't define.
        assert_eq!(
            attest(&[0, 7, 1, 0], Some(OWN), Uuid::new_v4()),
            Attested::Malformed
        );
    }

    #[test]
    fn leaving_stands_and_bad_states_are_dropped() {
        let me = Uuid::new_v4();
        let left = frame(&[update(&[(OWN, "null")])]);
        assert_eq!(
            entries(attest(&left, Some(OWN), me))[0].1,
            serde_json::Value::Null
        );

        for bad in [
            "[1,2]".to_string(),
            r#"{"user":"Sam"}"#.to_string(),
            "not json".to_string(),
            format!(r#"{{"preview":"{}"}}"#, "x".repeat(MAX_STATE_BYTES)),
        ] {
            let f = frame(&[update(&[(OWN, bad.as_str())])]);
            assert_eq!(attest(&f, Some(OWN), me), Attested::Empty, "{bad:.40}");
        }
    }

    #[test]
    fn a_session_pins_only_a_frame_naming_one_client() {
        assert_eq!(sole_client(&frame(&[update(&[(OWN, "{}")])])), Some(OWN));
        assert_eq!(
            sole_client(&frame(&[update(&[(OWN, "{}"), (OTHER, "{}")])])),
            None
        );
        assert_eq!(
            sole_client(&frame(&[update(&[(OWN, "{}")]), update(&[(OTHER, "{}")])])),
            None
        );
        assert_eq!(
            sole_client(&frame(&[Message::Sync(SyncMessage::Update(vec![0, 0]))])),
            None
        );
    }
}
