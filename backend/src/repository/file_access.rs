//! Who may load a ticket's stored files.
//!
//! A ticket file is either an attachment on a reply (or a PDF thumbnail stored
//! beside one) or an image in the ticket's notes. An attachment sits in the
//! `tickets/{id}/` folder of the ticket it was first stored for, which proves
//! nothing about the ticket it belongs to now: a merge moves the reply and
//! leaves the file where it was. So an attachment is resolved through its
//! `attachments` row to its reply, and the reply decides: it may be an
//! internal note only for a viewer who sees internal notes, and its ticket
//! must be one the viewer can see. A file under `tickets/` that no row
//! accounts for is refused. A notes image has no row and follows the ticket in
//! its path.
//!
//! The agent file route, the raw-mail route and the portal download all decide
//! here, so they cannot disagree about a file.

use diesel::prelude::*;
use uuid::Uuid;

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

/// A ticket file as [`locate`] found it in its workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocatedFile {
    /// A notes image, which follows its ticket.
    Note { ticket_id: i32 },
    /// The `attachments` rows the file's URL names in that workspace.
    Attachment { ids: Vec<i32> },
}

/// The workspace that holds `file`, and where in it, among the workspaces
/// `caller` is an active member of. Run elevated: it reveals a workspace and
/// row ids and decides nothing; [`can_load`] decides under that workspace's
/// pin. Only the caller's own workspaces are searched because a URL is not
/// unique across workspaces: one cloned within the same database keeps its
/// files' URLs.
pub fn locate(
    conn: &mut DbConnection,
    file: &TicketFile,
    caller: Uuid,
) -> QueryResult<Option<(i32, LocatedFile)>> {
    match file {
        TicketFile::Note { ticket_id } => Ok(crate::repository::tickets::workspace_id_by_id(
            conn, *ticket_id,
        )?
        .map(|ws| {
            (
                ws,
                LocatedFile::Note {
                    ticket_id: *ticket_id,
                },
            )
        })),
        TicketFile::Attachment { urls } => Ok(crate::repository::comments::attachment_locations(
            conn, urls, caller,
        )?
        .map(|(workspace_id, ids)| (workspace_id, LocatedFile::Attachment { ids }))),
    }
}

/// Whether `vis` may load the located file, on a connection pinned to its
/// workspace. Attachment rows are read again by id, under row security, so
/// the elevated lookup decided nothing but where to look.
pub fn can_load(
    conn: &mut DbConnection,
    vis: &VisibilityContext,
    file: &LocatedFile,
) -> QueryResult<bool> {
    match file {
        LocatedFile::Note { ticket_id } => can_view_ticket(conn, vis, *ticket_id),
        LocatedFile::Attachment { ids } => {
            let rows: Vec<Attachment> = attachments::table
                .filter(attachments::id.eq_any(ids))
                .load(conn)?;
            for row in &rows {
                if reply_for_viewer(conn, vis, row)?.is_some() {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

/// The attachment `attachment_id` and the reply it is on, if `vis` may load
/// it. `None` for an unknown id and for one the viewer can't see alike.
pub fn attachment_for_viewer(
    conn: &mut DbConnection,
    vis: &VisibilityContext,
    attachment_id: i32,
) -> QueryResult<Option<(Attachment, Comment)>> {
    let Some(row) = attachments::table
        .find(attachment_id)
        .first::<Attachment>(conn)
        .optional()?
    else {
        return Ok(None);
    };
    Ok(reply_for_viewer(conn, vis, &row)?.map(|comment| (row, comment)))
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

/// The reply an attachment is on, if `vis` may read it. One not on a reply
/// (a draft) is never a ticket file.
fn reply_for_viewer(
    conn: &mut DbConnection,
    vis: &VisibilityContext,
    row: &Attachment,
) -> QueryResult<Option<Comment>> {
    match row.comment_id {
        Some(comment_id) => comment_for_viewer(conn, vis, comment_id),
        None => Ok(None),
    }
}

/// A reply is readable when it is an internal note only for a viewer who sees
/// internal notes (staff, the same line as `CommentAudience::from_auth`), and
/// is on a ticket the viewer can see.
fn comment_admits(
    conn: &mut DbConnection,
    vis: &VisibilityContext,
    comment: &Comment,
) -> QueryResult<bool> {
    if comment.is_internal && !vis.sees_all() {
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
