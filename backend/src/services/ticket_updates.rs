//! Ticket writes that span more than one repository call: "New ticket", and
//! the connection wrappers the REST handlers and sync push run them on. The
//! rules engine assigns new tickets from their `ticket.created` event.

use uuid::Uuid;

use crate::db::DbConnection;
use crate::extractors::TenantConn;
use crate::models::{ArticleContent, NewArticleContent, NewTicket, Ticket};
use crate::repository;
use crate::sync::actor::ActorContext;
use crate::sync::session::with_actor_context;

/// Runs a closure in the caller's workspace, as its actor.
pub trait InWorkspace {
    fn run<T, E: From<diesel::result::Error>>(
        &mut self,
        f: impl FnOnce(&mut DbConnection) -> Result<T, E>,
    ) -> Result<T, E>;
}

impl InWorkspace for TenantConn {
    fn run<T, E: From<diesel::result::Error>>(
        &mut self,
        f: impl FnOnce(&mut DbConnection) -> Result<T, E>,
    ) -> Result<T, E> {
        TenantConn::run_result(self, f)
    }
}

/// A held connection and the actor to run as, for sync push.
pub struct ActorConn<'a> {
    pub conn: &'a mut DbConnection,
    pub actor: &'a ActorContext,
}

impl InWorkspace for ActorConn<'_> {
    fn run<T, E: From<diesel::result::Error>>(
        &mut self,
        f: impl FnOnce(&mut DbConnection) -> Result<T, E>,
    ) -> Result<T, E> {
        with_actor_context(self.conn, self.actor, f)
    }
}

/// What "New ticket" gave its caller.
pub enum BlankTicket {
    /// Their blank ticket from an earlier press, untouched since.
    Reused(Ticket),
    /// A new one, with its description if that was written.
    Made(Ticket, Option<ArticleContent>),
}

/// "New ticket" for `requester`, inside one transaction: their blank ticket
/// if they have one nothing has touched, otherwise a new one, marked blank.
/// The lock it takes first holds to the end of the transaction, so presses
/// that arrive together get the same ticket.
///
/// `submitted_via` is how a new one is stamped: `"api"` when an API token
/// asks (see `services::ticket_mail_redaction`).
pub fn blank_ticket(
    db: &mut ActorConn,
    requester: Uuid,
    submitted_via: Option<&str>,
) -> diesel::QueryResult<BlankTicket> {
    db.run(|conn| repository::tickets::lock_blank_ticket(conn, requester))?;
    if let Some(ticket) = db.run(|conn| repository::tickets::blank_ticket_for(conn, requester))? {
        return Ok(BlankTicket::Reused(ticket));
    }

    let default_state = db.run(repository::workflow_states::default_state)?;
    let new_ticket = NewTicket {
        title: "New Ticket".to_string(),
        workflow_state_id: default_state.id,
        requester_uuid: Some(requester),
        submitted_via: submitted_via.map(str::to_string),
        ..Default::default()
    };
    let ticket = db.run(|conn| repository::create_ticket(conn, new_ticket))?;

    // Its description; a ticket without one still opens.
    let article_content = db
        .run(|conn| {
            repository::create_article_content(
                conn,
                NewArticleContent {
                    ticket_id: ticket.id,
                    yjs_state_vector: None,
                    yjs_document: None,
                    yjs_client_id: None,
                },
            )
        })
        .ok();

    let ticket = db.run(|conn| repository::tickets::mark_blank(conn, ticket.id))?;
    Ok(BlankTicket::Made(ticket, article_content))
}
