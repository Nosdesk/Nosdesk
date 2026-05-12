//! Tag CRUD + per-ticket assignment handlers.
//!
//! Tag management (create / rename / archive) is admin-only;
//! assignment (PUT /tickets/:id/tags) is allowed for any
//! authenticated staff member, mirroring how comment-creation
//! permissions work.

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use tracing::error;

use crate::extractors::{AuthContext, TicketAccess};
use crate::handlers::{errors, helpers};
use crate::handlers::helpers::with_actor;
use crate::models::{NewTag, TagUpdate};
use crate::repository::tags as repo;
use crate::sync::actor::ActorContext;

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
    pool: web::Data<crate::db::Pool>,
    query: web::Query<ListTagsQuery>,
    _auth: AuthContext,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match repo::list_tags(&mut conn, query.include_archived) {
        Ok(tags) => HttpResponse::Ok().json(tags),
        Err(e) => {
            error!(error = %e, "list_tags failed");
            errors::internal("Failed to list tags")
        }
    }
}

pub async fn create_tag(
    pool: web::Data<crate::db::Pool>,
    body: web::Json<NewTag>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_admin() {
        return errors::forbidden("Only admins can create tags");
    }
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let actor_ctx = ActorContext::user(auth.user_uuid, None);
    match with_actor(&mut conn, &actor_ctx, |conn| repo::create_tag(conn, body.into_inner())) {
        Ok(tag) => HttpResponse::Ok().json(tag),
        Err(e) => {
            error!(error = %e, "create_tag failed");
            errors::internal("Failed to create tag")
        }
    }
}

pub async fn update_tag(
    pool: web::Data<crate::db::Pool>,
    params: web::Path<i32>,
    body: web::Json<TagUpdate>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_admin() {
        return errors::forbidden("Only admins can update tags");
    }
    let id = params.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let actor_ctx = ActorContext::user(auth.user_uuid, None);
    match with_actor(&mut conn, &actor_ctx, |conn| repo::update_tag(conn, id, body.into_inner())) {
        Ok(tag) => HttpResponse::Ok().json(tag),
        Err(e) => {
            error!(error = %e, "update_tag failed");
            errors::internal("Failed to update tag")
        }
    }
}

pub async fn archive_tag(
    pool: web::Data<crate::db::Pool>,
    params: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_admin() {
        return errors::forbidden("Only admins can archive tags");
    }
    let id = params.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let actor_ctx = ActorContext::user(auth.user_uuid, None);
    match with_actor(&mut conn, &actor_ctx, |conn| repo::archive_tag(conn, id)) {
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
    pool: web::Data<crate::db::Pool>,
    access: TicketAccess,
    body: web::Json<SetTicketTagsBody>,
) -> impl Responder {
    let TicketAccess { ticket_id, auth } = access;
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let actor_ctx = ActorContext::user(auth.user_uuid, None);
    match with_actor(&mut conn, &actor_ctx, |conn| {
        repo::set_tags_for_ticket(conn, ticket_id, &body.tag_ids, Some(auth.user_uuid))
    }) {
        Ok(tag_ids) => HttpResponse::Ok().json(serde_json::json!({ "tag_ids": tag_ids })),
        Err(e) => {
            error!(error = %e, ticket_id, "set_ticket_tags failed");
            errors::internal("Failed to update ticket tags")
        }
    }
}
