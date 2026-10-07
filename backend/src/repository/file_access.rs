//! Who may load a ticket's stored files.
//!
//! A ticket file is either an attachment on a reply (or a PDF thumbnail stored
//! beside one) or an image in the ticket's notes. An attachment sits in the
//! `tickets/{id}/` folder of the ticket it was first stored for, which proves
//! nothing about the ticket it belongs to now: a merge moves the reply and
//! leaves the file where it was. So an attachment is resolved through its
//! `attachments` row to its reply, and the reply decides. It must not be
//! removed, it may be an internal note only for a viewer who sees internal
//! notes, and its ticket must be one the viewer can see. A file under
//! `tickets/` that no row accounts for is refused. A notes image has no row
//! and follows the ticket in its path.
//!
//! The agent file route, the raw-mail route and the portal download all decide
//! here, so they cannot disagree about a file.

use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::{Attachment, Comment};
use crate::repository::ticket_visibility::{can_view_ticket, VisibilityContext};
use crate::schema::{attachments, comments};

/// A stored file under `tickets/`, by what proves who may load it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketFile {
    /// `tickets/{ticket_id}/notes/...`: a notes image, which follows its ticket.
    Note { ticket_id: i32 },
    /// Anything else: an attachment, found by any of these `attachments.url`
    /// values (its own, or for a PDF thumbnail, its PDF's).
    Attachment { urls: Vec<String> },
}

impl TicketFile {
    /// Classify a path relative to `tickets/`. The leading ticket id is read
    /// only to recognise a notes image; it decides nothing for an attachment.
    pub fn from_path(path: &str) -> Self {
        let mut parts = path.splitn(3, '/');
        if let (Some(id), Some("notes"), Some(_)) = (parts.next(), parts.next(), parts.next()) {
            if let Ok(ticket_id) = id.parse::<i32>() {
                return Self::Note { ticket_id };
            }
        }
        let urls = std::iter::once(path.to_string())
            .chain(crate::utils::pdf::pdf_paths_for_thumbnail(path))
            .map(|p| format!("/uploads/tickets/{p}"))
            .collect();
        Self::Attachment { urls }
    }
}

/// The workspace that owns `file`. Run elevated: it reveals a workspace id and
/// decides nothing else; [`can_load`] decides access under that workspace's pin.
pub fn owning_workspace(conn: &mut DbConnection, file: &TicketFile) -> QueryResult<Option<i32>> {
    match file {
        TicketFile::Note { ticket_id } => {
            crate::repository::tickets::workspace_id_by_id(conn, *ticket_id)
        }
        TicketFile::Attachment { urls } => {
            crate::repository::comments::attachment_workspace_id_by_urls(conn, urls)
        }
    }
}

/// Whether `vis` may load `file`, on a connection pinned to its workspace.
pub fn can_load(
    conn: &mut DbConnection,
    vis: &VisibilityContext,
    file: &TicketFile,
) -> QueryResult<bool> {
    match file {
        TicketFile::Note { ticket_id } => can_view_ticket(conn, vis, *ticket_id),
        TicketFile::Attachment { urls } => {
            let rows: Vec<Attachment> = attachments::table
                .filter(attachments::url.eq_any(urls))
                .load(conn)?;
            for row in &rows {
                if attachment_admits(conn, vis, row)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

/// The attachment `attachment_id`, if `vis` may load it. `None` for an unknown
/// id and for one the viewer can't see alike.
pub fn attachment_for_viewer(
    conn: &mut DbConnection,
    vis: &VisibilityContext,
    attachment_id: i32,
) -> QueryResult<Option<Attachment>> {
    let Some(row) = attachments::table
        .find(attachment_id)
        .first::<Attachment>(conn)
        .optional()?
    else {
        return Ok(None);
    };
    Ok(attachment_admits(conn, vis, &row)?.then_some(row))
}

/// The reply `comment_id`, if `vis` may read it. `None` for an unknown id and
/// for one the viewer can't see alike.
pub fn comment_for_viewer(
    conn: &mut DbConnection,
    vis: &VisibilityContext,
    comment_id: i32,
) -> QueryResult<Option<Comment>> {
    let Some(comment) = comments::table
        .find(comment_id)
        .first::<Comment>(conn)
        .optional()?
    else {
        return Ok(None);
    };
    Ok(comment_admits(conn, vis, &comment)?.then_some(comment))
}

/// An attachment is loadable when it is on a reply the viewer may read. One
/// not on a reply (a draft) is never a ticket file.
fn attachment_admits(
    conn: &mut DbConnection,
    vis: &VisibilityContext,
    row: &Attachment,
) -> QueryResult<bool> {
    let Some(comment_id) = row.comment_id else {
        return Ok(false);
    };
    Ok(comment_for_viewer(conn, vis, comment_id)?.is_some())
}

/// A reply is readable when it is not removed, is an internal note only for a
/// viewer who sees internal notes (staff, the same line as
/// `CommentAudience::from_auth`), and is on a ticket the viewer can see.
fn comment_admits(
    conn: &mut DbConnection,
    vis: &VisibilityContext,
    comment: &Comment,
) -> QueryResult<bool> {
    if comment.deleted_at.is_some() || (comment.is_internal && !vis.sees_all()) {
        return Ok(false);
    }
    can_view_ticket(conn, vis, comment.ticket_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notes_path_follows_its_ticket() {
        assert_eq!(
            TicketFile::from_path("12/notes/abc_paste.png"),
            TicketFile::Note { ticket_id: 12 }
        );
    }

    #[test]
    fn any_other_path_is_an_attachment_found_by_url() {
        assert_eq!(
            TicketFile::from_path("12/abc_scan.png"),
            TicketFile::Attachment {
                urls: vec!["/uploads/tickets/12/abc_scan.png".to_string()]
            }
        );
        // Inbound mail once stored attachments unfoldered.
        assert_eq!(
            TicketFile::from_path("abc_legacy.pdf"),
            TicketFile::Attachment {
                urls: vec!["/uploads/tickets/abc_legacy.pdf".to_string()]
            }
        );
        // A folder named `notes` that isn't under a ticket id is no notes image.
        assert!(matches!(
            TicketFile::from_path("x/notes/abc.png"),
            TicketFile::Attachment { .. }
        ));
    }

    #[test]
    fn a_pdf_thumbnail_is_found_by_its_pdf() {
        assert_eq!(
            TicketFile::from_path("12/abc_scan_thumb.webp"),
            TicketFile::Attachment {
                urls: vec![
                    "/uploads/tickets/12/abc_scan_thumb.webp".to_string(),
                    "/uploads/tickets/12/abc_scan.pdf".to_string(),
                    "/uploads/tickets/12/abc_scan.PDF".to_string(),
                ]
            }
        );
    }
}
