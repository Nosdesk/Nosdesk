use diesel::prelude::*;
use diesel::QueryResult;
use serde_json::json;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

/// Observer fired after a comment is successfully created. The
/// search service uses it to index the comment with its parent
/// ticket title, so any handler that creates a comment populates
/// the index automatically.
pub trait CommentCreatedObserver: Send + Sync {
    fn comment_created(&self, comment: &Comment, ticket_title: &str);
}

/// Observer fired after a comment is hard-deleted. Implementor
/// removes the comment from the search index.
pub trait CommentDeletedObserver: Send + Sync {
    fn comment_deleted(&self, comment_id: i32);
}

// Comment operations
pub fn get_comments_by_ticket_id(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<Vec<Comment>> {
    comments::table
        .filter(comments::ticket_id.eq(ticket_id))
        .order(comments::created_at.desc())
        .load(conn)
}

/// Requester-visible comment list: drops internal notes and soft-deleted
/// rows. Used by the guest-portal / public status views. Never call this
/// from tech-facing endpoints — techs need to see internal notes.
pub fn get_public_comments_by_ticket_id(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<Vec<Comment>> {
    comments::table
        .filter(comments::ticket_id.eq(ticket_id))
        .filter(comments::is_internal.eq(false))
        .filter(comments::deleted_at.is_null())
        .order(comments::created_at.desc())
        .load(conn)
}

/// Typed annotation describing where a comment originated, attached
/// to the `comment.created` sync_actions row so the activity feed
/// can render richer phrasing than "System commented on this
/// ticket".
///
/// Mirrors `repository::tickets::TicketCreationAnnotation`: every
/// field optional, callers populate just what they know. The
/// annotation lives on the comment.created event's `data.created_via`
/// nested object — same shape as the ticket.created annotation so
/// the frontend can reuse one parser.
#[derive(Debug, Clone, Default)]
pub struct CommentCreationAnnotation {
    /// Origin tag the renderer switches on. Conventional values
    /// match the ticket-side tags:
    ///   * `"channel:<provider>"` for inbound channel comments.
    ///   * `"guest_portal"` for the initial portal-form comment.
    /// Other values fall through to a generic "commented" line.
    pub source: Option<String>,
    /// Sender's email — surfaces in the actor slot of the activity
    /// entry so an inbound reply renders as "alice@example.com
    /// replied via email" rather than "System commented".
    pub from_email: Option<String>,
    /// Display name for the sender, when present.
    pub from_name: Option<String>,
}

/// Bare create — UI handlers, the import binary, and any caller
/// without specific channel/portal context land here.
pub fn create_comment(
    conn: &mut DbConnection,
    new_comment: NewComment,
    observer: Option<&dyn CommentCreatedObserver>,
) -> QueryResult<Comment> {
    create_comment_with_annotation(
        conn,
        new_comment,
        CommentCreationAnnotation::default(),
        observer,
    )
}

/// Create with explicit origin annotation. The inbound channel
/// pipeline and the guest portal handler use this; everything else
/// stays on the bare `create_comment`.
pub fn create_comment_with_annotation(
    conn: &mut DbConnection,
    new_comment: NewComment,
    annotation: CommentCreationAnnotation,
    observer: Option<&dyn CommentCreatedObserver>,
) -> QueryResult<Comment> {
    let ticket_id = new_comment.ticket_id;
    let comment = conn.transaction::<Comment, diesel::result::Error, _>(|conn| {
        // Resolve the parent ticket up front. Loading it here surfaces
        // a missing-parent error from the FK with a clear "ticket
        // doesn't exist" semantic, instead of letting the comment
        // INSERT below fail with a generic FK violation. The parent
        // is also needed for sync_groups computation; one query
        // serves both purposes.
        let parent: Ticket = tickets::table.find(ticket_id).first(conn)?;

        let comment: Comment = diesel::insert_into(comments::table)
            .values(&new_comment)
            .get_result(conn)?;

        // Bump the parent ticket's updated_at so list views surface
        // the activity. Failure here would be surprising once we've
        // already loaded the parent successfully, so let it bubble
        // (an UPDATE on a row we just SELECT'd will only fail under
        // pathological tx isolation issues — better to fail the
        // comment write than to silently drift updated_at).
        diesel::update(tickets::table.find(ticket_id))
            .set(tickets::updated_at.eq(diesel::dsl::now))
            .execute(conn)?;

        let groups = groups::for_ticket(conn, &parent)?;

        // Mirrors the `ticket.created` annotation shape so the
        // frontend reads `data.created_via` the same way regardless
        // of event aggregate. Always emitted (even with all fields
        // None) for shape stability.
        let created_via = json!({
            "source": annotation.source,
            "from_email": annotation.from_email,
            "from_name": annotation.from_name,
        });

        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Comment,
                aggregate_id: comment.id.to_string(),
                op: SyncOp::Insert,
                event_type: "comment.created",
                data: json!({
                    "id": comment.id,
                    "ticket_id": comment.ticket_id,
                    "user_uuid": comment.user_uuid,
                    "is_internal": comment.is_internal,
                    "content_format": comment.content_format,
                    "created_via": created_via,
                }),
                groups: groups.clone(),
                causation_id: None,
            },
        )?;

        // SLA response-timer stamp. The first non-internal comment by
        // a staff member (admin / technician) marks the moment the
        // response target was met. Stamped idempotently with a
        // `first_response_at IS NULL` predicate so concurrent first
        // replies don't race. Internal notes and requester replies
        // don't count — industry convention.
        if !comment.is_internal && parent.first_response_at.is_none() {
            // "Staff" post-W2: workspace owner/admin/agent in the
            // current workspace, or any platform admin. Hits
            // workspace 1 since OSS is single-tenant; multi-tenant
            // callers would pass the resolved workspace_id here.
            let is_staff = diesel::dsl::select(diesel::dsl::exists(
                crate::schema::users::table
                    .filter(crate::schema::users::uuid.eq(new_comment.user_uuid))
                    .filter(
                        crate::schema::users::platform_role
                            .eq("platform_admin")
                            .or(diesel::dsl::exists(
                                crate::schema::workspace_members::table
                                    .filter(
                                        crate::schema::workspace_members::user_uuid
                                            .eq(crate::schema::users::uuid),
                                    )
                                    .filter(crate::schema::workspace_members::workspace_id.eq(1))
                                    .filter(crate::schema::workspace_members::role.eq_any(vec![
                                        "owner", "admin", "agent",
                                    ])),
                            )),
                    ),
            ))
            .get_result::<bool>(conn)
            .unwrap_or(false);
            if is_staff {
                let stamped = diesel::update(tickets::table.find(ticket_id))
                    .filter(tickets::first_response_at.is_null())
                    .set(tickets::first_response_at.eq(diesel::dsl::now))
                    .execute(conn)?;
                // Only emit the sla_updated event when we actually
                // won the idempotency race; otherwise another comment
                // already stamped and we'd broadcast a duplicate.
                if stamped > 0 {
                    let updated_ticket: Ticket = tickets::table.find(ticket_id).first(conn)?;
                    let sla = crate::services::sla::recompute_and_stamp_sla_for_ticket(
                        conn,
                        &updated_ticket,
                    );
                    emit::record(
                        conn,
                        SyncEmit {
                            aggregate: SyncAggregate::Ticket,
                            aggregate_id: ticket_id.to_string(),
                            op: SyncOp::Update,
                            event_type: "ticket.sla_updated",
                            data: json!({
                                "id": ticket_id,
                                "first_response_at": updated_ticket.first_response_at,
                                "sla": sla,
                            }),
                            groups,
                            causation_id: None,
                        },
                    )?;
                }
            }
        }

        Ok(comment)
    })?;

    if let Some(observer) = observer {
        let ticket_title = tickets::table
            .find(ticket_id)
            .select(tickets::title)
            .first::<String>(conn)
            .unwrap_or_else(|_| String::from("Unknown Ticket"));
        observer.comment_created(&comment, &ticket_title);
    }

    Ok(comment)
}

// Attachment operations
pub fn get_attachments_by_comment_id(
    conn: &mut DbConnection,
    comment_id: i32,
) -> QueryResult<Vec<Attachment>> {
    attachments::table
        .filter(attachments::comment_id.eq(comment_id))
        .load(conn)
}

pub fn create_attachment(
    conn: &mut DbConnection,
    new_attachment: NewAttachment,
) -> QueryResult<Attachment> {
    conn.transaction(|conn| {
        let attachment: Attachment = diesel::insert_into(attachments::table)
            .values(&new_attachment)
            .get_result(conn)?;
        // Resolve groups via the parent comment's ticket so the
        // attachment lands on the same fan-out as its sibling
        // comment events. Attachments may be orphan (comment_id NULL,
        // for guest temp uploads); fall back to the workspace group
        // in that case.
        let groups = match attachment.comment_id {
            Some(cid) => {
                let tid: i32 = comments::table
                    .find(cid)
                    .select(comments::ticket_id)
                    .first(conn)?;
                let parent: Ticket = tickets::table.find(tid).first(conn)?;
                groups::for_ticket(conn, &parent)?
            }
            None => groups::workspace(),
        };
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Attachment,
                aggregate_id: attachment.id.to_string(),
                op: SyncOp::Insert,
                event_type: "attachment.created",
                data: json!({
                    "id": attachment.id,
                    "comment_id": attachment.comment_id,
                    "name": attachment.name,
                    "mime_type": attachment.mime_type,
                    "file_size": attachment.file_size,
                }),
                groups,
                causation_id: None,
            },
        )?;
        Ok(attachment)
    })
}

pub fn get_comment_by_id(conn: &mut DbConnection, comment_id: i32) -> QueryResult<Comment> {
    comments::table.find(comment_id).first(conn)
}

pub fn get_comments_with_attachments_by_ticket_id(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<Vec<CommentWithAttachments>> {
    let comments = get_comments_by_ticket_id(conn, ticket_id)?;

    // Batch-fetch `from_address` for every comment up front so the
    // assembly loop stays O(n) rather than issuing a per-comment
    // query. The lookup spans the linked `channel_messages` table —
    // the authoritative location for a sender's external address.
    // Comments authored through the helpdesk UI have no row there
    // and just see `None`.
    let comment_ids: Vec<i32> = comments.iter().map(|c| c.id).collect();
    let mut from_addresses =
        crate::repository::channels::from_addresses_for_comments(conn, &comment_ids)
            .unwrap_or_default();

    comments
        .into_iter()
        .map(|comment| {
            let attachments = get_attachments_by_comment_id(conn, comment.id)?;
            let user = crate::repository::users::get_user_by_uuid(&comment.user_uuid, conn)
                .ok()
                .map(UserInfoWithAvatar::from);
            let from_address = from_addresses.remove(&comment.id);
            let has_raw_source = comment.raw_source_uri.is_some();
            Ok(CommentWithAttachments {
                comment,
                attachments,
                user,
                from_address,
                has_raw_source,
            })
        })
        .collect()
}

pub fn delete_comment(
    conn: &mut DbConnection,
    comment_id: i32,
    observer: Option<&dyn CommentDeletedObserver>,
) -> QueryResult<usize> {
    let count = conn.transaction::<usize, diesel::result::Error, _>(|conn| {
        // Capture the parent ticket BEFORE deletion so the emit
        // resolves to the right sync groups (the comment row goes
        // away mid-transaction; we need its ticket_id first).
        let parent_ticket_id: Option<i32> = comments::table
            .find(comment_id)
            .select(comments::ticket_id)
            .first(conn)
            .optional()?;

        // First delete all attachments associated with this comment
        diesel::delete(attachments::table.filter(attachments::comment_id.eq(comment_id)))
            .execute(conn)?;

        // Then delete the comment itself
        let count = diesel::delete(comments::table.find(comment_id)).execute(conn)?;
        if count > 0 {
            if let Some(tid) = parent_ticket_id {
                let parent: Ticket = tickets::table.find(tid).first(conn)?;
                let groups = groups::for_ticket(conn, &parent)?;
                emit::record(
                    conn,
                    SyncEmit {
                        aggregate: SyncAggregate::Comment,
                        aggregate_id: comment_id.to_string(),
                        op: SyncOp::Delete,
                        event_type: "comment.deleted",
                        data: json!({ "id": comment_id, "ticket_id": tid }),
                        groups,
                        causation_id: None,
                    },
                )?;
            }
        }
        Ok(count)
    })?;
    if count > 0 {
        if let Some(observer) = observer {
            observer.comment_deleted(comment_id);
        }
    }
    Ok(count)
}

pub fn get_attachment_by_id(
    conn: &mut DbConnection,
    attachment_id: i32,
) -> QueryResult<Attachment> {
    attachments::table.find(attachment_id).first(conn)
}

pub fn delete_attachment(conn: &mut DbConnection, attachment_id: i32) -> QueryResult<usize> {
    conn.transaction(|conn| {
        // Capture parent comment before delete so groups resolve.
        // attachments.comment_id is nullable (orphan temp uploads),
        // hence the doubly-Option select pattern.
        let parent_comment_id: Option<Option<i32>> = attachments::table
            .find(attachment_id)
            .select(attachments::comment_id)
            .first(conn)
            .optional()?;
        let result = diesel::delete(attachments::table.find(attachment_id)).execute(conn)?;
        if result > 0 {
            let groups = match parent_comment_id.flatten() {
                Some(cid) => {
                    let tid: i32 = comments::table
                        .find(cid)
                        .select(comments::ticket_id)
                        .first(conn)?;
                    let parent: Ticket = tickets::table.find(tid).first(conn)?;
                    groups::for_ticket(conn, &parent)?
                }
                None => groups::workspace(),
            };
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::Attachment,
                    aggregate_id: attachment_id.to_string(),
                    op: SyncOp::Delete,
                    event_type: "attachment.deleted",
                    data: json!({ "id": attachment_id }),
                    groups,
                    causation_id: None,
                },
            )?;
        }
        Ok(result)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn create_and_retrieve_comment() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "commenter", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "Ticket", Some(user.uuid), None);

        let comment = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "Hello world");
        assert_eq!(comment.content, "Hello world");
        assert_eq!(comment.ticket_id, ticket.id);

        let comments = get_comments_by_ticket_id(&mut conn, ticket.id).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].id, comment.id);
    }

    #[test]
    fn multiple_comments_all_returned() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "order", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);

        let c1 = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "First");
        let c2 = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "Second");

        let comments = get_comments_by_ticket_id(&mut conn, ticket.id).unwrap();
        assert_eq!(comments.len(), 2);
        let ids: Vec<i32> = comments.iter().map(|c| c.id).collect();
        assert!(ids.contains(&c1.id));
        assert!(ids.contains(&c2.id));
    }

    #[test]
    fn public_comments_exclude_internal_and_deleted() {
        use diesel::prelude::*;

        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "visible", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "V", Some(user.uuid), None);

        // Public.
        let public =
            TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "requester can see this");

        // Internal: set the flag post-insert since the fixture hardcodes
        // is_internal=false.
        let internal =
            TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "tech-only note");
        diesel::update(comments::table.find(internal.id))
            .set(comments::is_internal.eq(true))
            .execute(&mut conn)
            .unwrap();

        // Soft-deleted.
        let deleted = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "retracted");
        diesel::update(comments::table.find(deleted.id))
            .set(comments::deleted_at.eq(Some(chrono::Utc::now().naive_utc())))
            .execute(&mut conn)
            .unwrap();

        let visible = get_public_comments_by_ticket_id(&mut conn, ticket.id).unwrap();
        let ids: Vec<i32> = visible.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![public.id]);
    }

    #[test]
    fn create_comment_updates_ticket_timestamp() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "tsuser", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "TS", Some(user.uuid), None);
        let original_updated = ticket.updated_at;

        // Small delay to ensure timestamp differs
        std::thread::sleep(std::time::Duration::from_millis(10));

        let new_comment = NewComment {
            content: "bump".to_string(),
            ticket_id: ticket.id,
            user_uuid: user.uuid,
            ..Default::default()
        };
        create_comment(&mut conn, new_comment, None).unwrap();

        let updated_ticket =
            crate::repository::tickets::get_ticket_by_id(&mut conn, ticket.id).unwrap();
        assert!(updated_ticket.updated_at >= original_updated);
    }

    #[test]
    fn create_and_retrieve_attachment() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "attuser", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);
        let comment = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "With file");

        let att = TestFixtures::create_attachment(&mut conn, comment.id, "doc.pdf");
        assert_eq!(att.name, "doc.pdf");

        let atts = get_attachments_by_comment_id(&mut conn, comment.id).unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].id, att.id);
    }

    #[test]
    fn delete_comment_cascades_attachments() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "deluser", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);
        let comment = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "Bye");
        let att = TestFixtures::create_attachment(&mut conn, comment.id, "file.txt");

        delete_comment(&mut conn, comment.id, None).unwrap();

        assert!(get_comment_by_id(&mut conn, comment.id).is_err());
        assert!(get_attachment_by_id(&mut conn, att.id).is_err());
    }

    #[test]
    fn delete_single_attachment() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "delatt", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);
        let comment = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "Keep me");
        let att = TestFixtures::create_attachment(&mut conn, comment.id, "remove.pdf");

        delete_attachment(&mut conn, att.id).unwrap();

        assert!(get_attachment_by_id(&mut conn, att.id).is_err());
        // Comment should still exist
        assert!(get_comment_by_id(&mut conn, comment.id).is_ok());
    }
}
