//! Mail about a ticket a stranger filed carries none of the stranger's text
//! until the requester proves the address.
//!
//! A ticket is of stranger provenance when nobody has vouched for the person
//! who wrote it: a request-form ticket filed while the form didn't ask for
//! confirmation, or a ticket an API token created (a website relaying a form,
//! say). Its title, the requester's name, the message, field values and file
//! names were typed by whoever filled in the form, and the address they gave
//! may be anyone's. While the requester hasn't proven that address since the
//! ticket was filed (a sign-in link or code, or a confirmation POST; see
//! `repository::user_emails::mark_primary_verified`), automatic mail about the
//! ticket to anyone outside the team names it only as "request #N".
//!
//! [`ticket_mail_redaction`] is the one place that decides. Every function
//! that queues mail about a ticket calls it for each recipient and composes
//! from its answer; the list is in `repository::outbound_emails`. Mail to the
//! team is never redacted: staff see the ticket anyway.

use diesel::QueryResult;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::Ticket;

/// Who a message about a ticket goes to.
#[derive(Debug, Clone, Copy)]
pub enum Recipient<'a> {
    /// A person, by account.
    User(Uuid),
    /// An address, which may belong to an account or to nobody.
    Address(&'a str),
}

/// What mail about a ticket may show its recipient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketMailText {
    /// The ticket's own text: its title, the requester's name, the message.
    Full,
    /// Only the ticket's number and the workspace's own wording.
    Redacted,
}

impl TicketMailText {
    pub fn is_redacted(self) -> bool {
        self == Self::Redacted
    }
}

/// Whether the ticket's text came from someone nobody has vouched for: a
/// request-form ticket filed without confirmation, or one an API token filed.
pub fn stranger_provenance(ticket: &Ticket) -> bool {
    match ticket.submitted_via.as_deref() {
        Some(crate::models::SUBMITTED_VIA_API) => true,
        // A confirmed request-form ticket is `verified`. No state at all is a
        // form that didn't ask; one still `pending` has nobody vouching for it
        // either, though nothing mails about it.
        Some(crate::models::SUBMITTED_VIA_GUEST) => {
            ticket.verification_state.as_deref() != Some("verified")
        }
        _ => false,
    }
}

/// What mail about `ticket` to `recipient` may show. Fails closed: a read
/// that fails redacts. `conn` must be pinned to the ticket's workspace.
pub fn ticket_mail_redaction(
    conn: &mut DbConnection,
    ticket: &Ticket,
    recipient: Recipient<'_>,
) -> TicketMailText {
    if !stranger_provenance(ticket) {
        return TicketMailText::Full;
    }
    match decide(conn, ticket, recipient) {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!(
                ticket_id = ticket.id,
                error = ?e,
                "ticket mail redaction: check failed; redacting"
            );
            TicketMailText::Redacted
        }
    }
}

/// [`ticket_mail_redaction`] for a ticket known by id, as notification
/// delivery knows it. A ticket that can't be read, or is gone, redacts: the
/// email was composed from it while it was there.
pub fn ticket_mail_redaction_by_id(
    conn: &mut DbConnection,
    ticket_id: i32,
    recipient: Recipient<'_>,
) -> TicketMailText {
    match crate::repository::tickets::get_ticket_by_id(conn, ticket_id) {
        Ok(ticket) => ticket_mail_redaction(conn, &ticket, recipient),
        Err(e) => {
            tracing::warn!(ticket_id, error = ?e, "ticket mail redaction: ticket unreadable; redacting");
            TicketMailText::Redacted
        }
    }
}

fn decide(
    conn: &mut DbConnection,
    ticket: &Ticket,
    recipient: Recipient<'_>,
) -> QueryResult<TicketMailText> {
    if on_team(conn, ticket.workspace_id, recipient)? || requester_proven_since(conn, ticket)? {
        Ok(TicketMailText::Full)
    } else {
        Ok(TicketMailText::Redacted)
    }
}

/// Whether the recipient works tickets in the workspace. An address works
/// tickets when any account holding it does.
fn on_team(
    conn: &mut DbConnection,
    workspace_id: i32,
    recipient: Recipient<'_>,
) -> QueryResult<bool> {
    let people: Vec<Uuid> = match recipient {
        Recipient::User(uuid) => vec![uuid],
        Recipient::Address(address) => crate::repository::user_emails::holders_of(conn, address)?
            .into_iter()
            .map(|e| e.user_uuid)
            .collect(),
    };
    for uuid in people {
        let Ok(user) = crate::repository::users::find_active_by_uuid(&uuid, conn) else {
            continue;
        };
        if crate::repository::user_helpers::user_can_handle_tickets_in(conn, workspace_id, &user) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the requester's address has proved itself since the ticket was
/// filed. Proof from before it doesn't count: a relay can file a ticket for
/// anyone's address, including one that signed in last year.
fn requester_proven_since(conn: &mut DbConnection, ticket: &Ticket) -> QueryResult<bool> {
    let Some(requester) = ticket.requester_uuid else {
        return Ok(false);
    };
    let proven = crate::repository::user_emails::primary_proven_at(conn, &requester)?;
    Ok(proven.is_some_and(|at| at.naive_utc() >= ticket.created_at))
}

/// `ticket` with the text a stranger supplied replaced, for rendering a
/// template that names it: the title becomes "#N". The requester's name is the
/// caller's to leave out.
pub fn redacted_copy(ticket: &Ticket) -> Ticket {
    Ticket {
        title: format!("#{}", ticket.number),
        ..ticket.clone()
    }
}
