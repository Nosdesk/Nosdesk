//! Repository for the documentation_page_tickets join table.
//!
//! Many-to-many between docs and tickets, with a `link_type` that
//! distinguishes "this doc resolved that ticket" from "this doc is
//! referenced from that ticket". Both directions of lookup live
//! here so callers can answer either question with one query.

use crate::db::DbConnection;
use crate::models::{DocumentationPageTicket, NewDocumentationPageTicket};
use crate::schema::documentation_page_tickets;
use diesel::prelude::*;
use diesel::result::Error;
use uuid::Uuid;

pub const LINK_RESOLVES: &str = "resolves";
pub const LINK_REFERENCES: &str = "references";

/// Validate a link_type string against the CHECK constraint enforced
/// by the database. Returned as Result so handlers can surface a
/// clean 400 instead of a generic database error.
pub fn validate_link_type(value: &str) -> Result<(), &'static str> {
    match value {
        LINK_RESOLVES | LINK_REFERENCES => Ok(()),
        _ => Err("link_type must be 'resolves' or 'references'"),
    }
}

/// Insert (or upsert) a doc<->ticket link. If the pair already
/// exists we update the link_type to whatever was just supplied,
/// since the latest action expresses the latest intent.
pub fn upsert_link(
    conn: &mut DbConnection,
    page_id: i32,
    ticket_id: i32,
    link_type: &str,
    created_by: Option<Uuid>,
) -> Result<DocumentationPageTicket, Error> {
    let row = NewDocumentationPageTicket {
        page_id,
        ticket_id,
        link_type: link_type.to_string(),
        created_by,
    };
    diesel::insert_into(documentation_page_tickets::table)
        .values(&row)
        .on_conflict((
            documentation_page_tickets::page_id,
            documentation_page_tickets::ticket_id,
        ))
        .do_update()
        .set(documentation_page_tickets::link_type.eq(link_type.to_string()))
        .get_result(conn)
}

pub fn delete_link(
    conn: &mut DbConnection,
    page_id_arg: i32,
    ticket_id_arg: i32,
) -> Result<usize, Error> {
    diesel::delete(
        documentation_page_tickets::table
            .filter(documentation_page_tickets::page_id.eq(page_id_arg))
            .filter(documentation_page_tickets::ticket_id.eq(ticket_id_arg)),
    )
    .execute(conn)
}

/// All links for a single page (used by the doc detail view to show
/// "Resolved N tickets / Referenced from M tickets").
pub fn links_for_page(
    conn: &mut DbConnection,
    page_id_arg: i32,
) -> Result<Vec<DocumentationPageTicket>, Error> {
    documentation_page_tickets::table
        .filter(documentation_page_tickets::page_id.eq(page_id_arg))
        .order_by(documentation_page_tickets::created_at.desc())
        .load::<DocumentationPageTicket>(conn)
}

/// All links for a single ticket (used by the ticket "See also"
/// panel to show docs).
pub fn links_for_ticket(
    conn: &mut DbConnection,
    ticket_id_arg: i32,
) -> Result<Vec<DocumentationPageTicket>, Error> {
    documentation_page_tickets::table
        .filter(documentation_page_tickets::ticket_id.eq(ticket_id_arg))
        .order_by(documentation_page_tickets::created_at.desc())
        .load::<DocumentationPageTicket>(conn)
}

/// Pick any 'resolves'-tier ticket for a page. Used by the
/// resolve_yjs_document fallback when a page predates the dedicated
/// yjs_document column and only has its content via the ticket's
/// article_content row. Returns the most recently created link.
pub fn most_recent_resolves_ticket_id(
    conn: &mut DbConnection,
    page_id_arg: i32,
) -> Result<Option<i32>, Error> {
    documentation_page_tickets::table
        .filter(documentation_page_tickets::page_id.eq(page_id_arg))
        .filter(documentation_page_tickets::link_type.eq(LINK_RESOLVES))
        .order_by(documentation_page_tickets::created_at.desc())
        .select(documentation_page_tickets::ticket_id)
        .first::<i32>(conn)
        .optional()
}
