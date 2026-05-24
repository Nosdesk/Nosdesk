//! Tag CRUD + per-ticket assignment handlers.
//!
//! Tag management (create / rename / archive) is admin-only;
//! assignment (PUT /tickets/:id/tags) is allowed for any
//! authenticated staff member, mirroring how comment-creation
//! permissions work.

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use tracing::error;

use crate::extractors::{AuthContext, TenantConn, TicketAccess};
use crate::handlers::errors;
use crate::models::{NewTag, TagUpdate};
use crate::repository::tags as repo;

#[derive(Debug, Deserialize)]
pub struct ListTagsQuery {
    /// When `true`, archived tags are included in the response.
    /// Defaults to `false` so the picker stays clean. The tag
    /// management UI passes `true` to surface archived tags for
    /// restore.
    #[serde(default)]
    pub include_archived: bool,
}

pub async fn list_tags(
    mut tc: TenantConn,
    query: web::Query<ListTagsQuery>,
    _auth: AuthContext,
) -> impl Responder {
    let include_archived = query.include_archived;
    match tc.run(|conn| repo::list_tags(conn, include_archived)) {
        Ok(tags) => HttpResponse::Ok().json(tags),
        Err(e) => {
            error!(error = %e, "list_tags failed");
            errors::internal("Failed to list tags")
        }
    }
}

pub async fn create_tag(
    mut tc: TenantConn,
    body: web::Json<NewTag>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_admin() {
        return errors::forbidden("Only admins can create tags");
    }
    match tc.run(|conn| repo::create_tag(conn, body.into_inner())) {
        Ok(tag) => HttpResponse::Ok().json(tag),
        Err(e) => {
            error!(error = %e, "create_tag failed");
            errors::internal("Failed to create tag")
        }
    }
}

pub async fn update_tag(
    mut tc: TenantConn,
    params: web::Path<i32>,
    body: web::Json<TagUpdate>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_admin() {
        return errors::forbidden("Only admins can update tags");
    }
    let id = params.into_inner();
    match tc.run(|conn| repo::update_tag(conn, id, body.into_inner())) {
        Ok(tag) => HttpResponse::Ok().json(tag),
        Err(e) => {
            error!(error = %e, "update_tag failed");
            errors::internal("Failed to update tag")
        }
    }
}

pub async fn archive_tag(
    mut tc: TenantConn,
    params: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_admin() {
        return errors::forbidden("Only admins can archive tags");
    }
    let id = params.into_inner();
    match tc.run(|conn| repo::archive_tag(conn, id)) {
        Ok(tag) => HttpResponse::Ok().json(tag),
        Err(e) => {
            error!(error = %e, "archive_tag failed");
            errors::internal("Failed to archive tag")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SetTicketTagsBody {
    /// Replace the ticket's tag set with this list. Empty array
    /// clears all tags. Same shape regardless of whether the
    /// caller is adding or removing — the repository computes
    /// the diff so the API stays simple.
    pub tag_ids: Vec<i32>,
}

pub async fn set_ticket_tags(
    mut tc: TenantConn,
    access: TicketAccess,
    body: web::Json<SetTicketTagsBody>,
) -> impl Responder {
    let TicketAccess { ticket_id, auth } = access;
    let tag_ids = body.tag_ids.clone();
    let actor_uuid = auth.user_uuid;
    match tc.run(|conn| repo::set_tags_for_ticket(conn, ticket_id, &tag_ids, Some(actor_uuid))) {
        Ok(tag_ids) => HttpResponse::Ok().json(serde_json::json!({ "tag_ids": tag_ids })),
        Err(e) => {
            error!(error = %e, ticket_id, "set_ticket_tags failed");
            errors::internal("Failed to update ticket tags")
        }
    }
}
