//! Automatic mail about a ticket carries its text (the title, the requester's
//! name, the message, field values, file names) to someone outside the team
//! only when that person has a reason to see it.
//!
//! A ticket is of stranger provenance when nobody has vouched for the person
//! who wrote it: a request-form ticket filed without confirmation, or a ticket
//! an API token created (a website relaying a form, say). Its text was typed by
//! whoever filled in the form, and the address it names may be anyone's.
//!
//! Someone outside the team gets the full text when:
//! - they have written on the ticket themselves since it was filed (a portal
//!   reply, or an emailed reply the channel accepted), on any ticket; or
//! - the ticket has no stranger provenance and they are its requester, or the
//!   team put them on it (`ticket_watchers.added_by` is staff, or a system
//!   write such as inbound mail copying in a Cc), or they put themselves on
//!   it (following it), or they aren't on it at all (an approver or a person
//!   a rule names: the team chose them).
//!
//! Otherwise the mail names it only as "request #N". A person a requester
//! added gets the number until they write on it themselves, so a requester
//! can't use the form to mail its text to whoever they like. Proof of the
//! address alone doesn't count: it isn't about this ticket, and `proven_at` is
//! the same for every workspace.
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
        Some(crate::models::SUBMITTED_VIA_API | crate::models::CSV_IMPORT_BY_TOKEN) => true,
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
    let people = people_of(conn, recipient)?;
    if on_team(conn, ticket.workspace_id, &people)? || engaged(conn, ticket, &people)? {
        return Ok(TicketMailText::Full);
    }
    if stranger_provenance(ticket) {
        return Ok(TicketMailText::Redacted);
    }
    if ticket.requester_uuid.is_some_and(|r| people.contains(&r)) {
        return Ok(TicketMailText::Full);
    }
    // On the ticket: only if the team, a system write or they themselves put
    // them there (following a ticket isn't someone else choosing to mail them).
    let added_by = watchers_added_by(conn, ticket.id, &people)?;
    if added_by.is_empty() {
        return Ok(TicketMailText::Full);
    }
    for (person, by) in added_by {
        let Some(by) = by else {
            return Ok(TicketMailText::Full);
        };
        if by == person || on_team(conn, ticket.workspace_id, &[by])? {
            return Ok(TicketMailText::Full);
        }
    }
    Ok(TicketMailText::Redacted)
}

/// The accounts a recipient stands for: the account, or every account holding
/// the address.
fn people_of(conn: &mut DbConnection, recipient: Recipient<'_>) -> QueryResult<Vec<Uuid>> {
    Ok(match recipient {
        Recipient::User(uuid) => vec![uuid],
        Recipient::Address(address) => crate::repository::user_emails::holders_of(conn, address)?
            .into_iter()
            .map(|e| e.user_uuid)
            .collect(),
    })
}

/// Whether any of `people` works tickets in the workspace.
fn on_team(conn: &mut DbConnection, workspace_id: i32, people: &[Uuid]) -> QueryResult<bool> {
    for uuid in people {
        let Ok(user) = crate::repository::users::find_active_by_uuid(uuid, conn) else {
            continue;
        };
        if crate::repository::user_helpers::user_can_handle_tickets_in(conn, workspace_id, &user) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether any of `people` has written a public comment on the ticket since
/// it was filed. The description a form files with the ticket is written in
/// the same transaction, at the same instant, so it doesn't count; nor does
/// viewing the ticket, which a link a mail scanner opens can do. A comment
/// that arrived by email counts only when the message passed DMARC
/// (`channel_metadata.sender_auth`, which the inbound pipeline stamps): a
/// forged `From` must not stand for the address it names. Other comments
/// (the portal, a chat channel) count as they are.
fn engaged(conn: &mut DbConnection, ticket: &Ticket, people: &[Uuid]) -> QueryResult<bool> {
    use crate::schema::comments as c;
    use diesel::prelude::*;
    if people.is_empty() {
        return Ok(false);
    }
    diesel::select(diesel::dsl::exists(
        c::table
            .filter(c::ticket_id.eq(ticket.id))
            .filter(c::user_uuid.eq_any(people))
            .filter(c::is_internal.eq(false))
            .filter(c::deleted_at.is_null())
            .filter(c::created_at.gt(ticket.created_at))
            .filter(diesel::dsl::sql::<diesel::sql_types::Bool>(
                "(channel_metadata->>'provider' IS NULL \
                  OR channel_metadata->>'provider' NOT LIKE 'email%' \
                  OR channel_metadata->>'sender_auth' = 'pass')",
            )),
    ))
    .get_result(conn)
}

/// Who put each of `people` on the ticket, one entry per watcher row: the
/// person, and who added them (`None` for a system write or a row from before
/// it was recorded). Empty when none of them is on it.
fn watchers_added_by(
    conn: &mut DbConnection,
    ticket_id: i32,
    people: &[Uuid],
) -> QueryResult<Vec<(Uuid, Option<Uuid>)>> {
    use crate::schema::ticket_watchers as w;
    use diesel::prelude::*;
    if people.is_empty() {
        return Ok(Vec::new());
    }
    w::table
        .filter(w::ticket_id.eq(ticket_id))
        .filter(w::user_uuid.eq_any(people))
        .select((w::user_uuid, w::added_by))
        .load(conn)
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
