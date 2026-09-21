use actix_multipart::Multipart;
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use bcrypt::DEFAULT_COST;
use diesel::prelude::*;
use futures::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::errors::{self, ApiError};
use crate::extractors::{TenantConn, WorkspaceContext};
use crate::handlers::helpers;
use crate::models::{UserResponse, UserUpdate, UserUpdateWithPassword};
use crate::repository;
use crate::repository::user_emails as user_emails_repo;
use crate::services::search::indexing_tasks;
use crate::services::search::SearchService;
use crate::utils;
use crate::utils::email_branding::get_email_branding;
use crate::utils::rbac::is_platform_admin;

pub fn config(cfg: &mut web::ServiceConfig) {
    // Note: Specific routes must come BEFORE generic {uuid} routes to avoid matching conflicts
    cfg.route("/users", web::get().to(crate::handlers::get_users))
        .route(
            "/users/paginated",
            web::get().to(crate::handlers::get_paginated_users),
        )
        .route(
            "/users/batch",
            web::post().to(crate::handlers::get_users_batch),
        )
        .route("/users/bulk", web::post().to(crate::handlers::bulk_users))
        .route(
            "/users/cleanup-images",
            web::post().to(crate::handlers::cleanup_stale_images),
        )
        .route(
            "/users/regenerate-thumbnails",
            web::post().to(crate::handlers::regenerate_avatar_thumbnails),
        )
        .route(
            "/files/cleanup-temp",
            web::post().to(crate::handlers::cleanup_temp_files),
        )
        .route(
            "/users/auth-identities",
            web::get().to(crate::handlers::get_user_auth_identities),
        )
        .route(
            "/users/auth-identities/{id}",
            web::delete().to(crate::handlers::delete_user_auth_identity),
        )
        .route("/users", web::post().to(crate::handlers::create_user))
        .route(
            "/users/{uuid}",
            web::get().to(crate::handlers::get_user_by_uuid),
        )
        .route(
            "/users/{uuid}",
            web::put().to(crate::handlers::update_user_by_uuid),
        )
        .route(
            "/users/{uuid}",
            web::delete().to(crate::handlers::delete_user),
        )
        .route(
            "/users/{uuid}/restore",
            web::post().to(crate::handlers::restore_user),
        )
        .route(
            "/users/{uuid}/purge",
            web::delete().to(crate::handlers::purge_user_now),
        )
        .route(
            "/users/{uuid}/image",
            web::post().to(crate::handlers::upload_user_image),
        )
        .route(
            "/users/{uuid}/emails",
            web::get().to(crate::handlers::get_user_emails),
        )
        .route(
            "/users/{uuid}/emails",
            web::post().to(crate::handlers::add_user_email),
        )
        .route(
            "/users/{uuid}/emails/{email_id}",
            web::put().to(crate::handlers::update_user_email),
        )
        .route(
            "/users/{uuid}/emails/{email_id}",
            web::delete().to(crate::handlers::delete_user_email),
        )
        .route(
            "/users/{uuid}/emails/{email_id}/resend-verification",
            web::post().to(crate::handlers::resend_user_email_verification),
        )
        // User contact profile (standard cols + custom-field values).
        .route(
            "/users/{uuid}/profile-fields",
            web::get().to(crate::handlers::user_contact::get_user_profile_fields),
        )
        .route(
            "/users/{uuid}/profile-fields",
            web::put().to(crate::handlers::user_contact::set_user_profile_fields),
        )
        // Multi-valued contact: phones + addresses (self or admin; synced rows read-only).
        .route(
            "/users/{uuid}/phones",
            web::get().to(crate::handlers::user_contact::list_user_phones),
        )
        .route(
            "/users/{uuid}/phones",
            web::post().to(crate::handlers::user_contact::add_user_phone),
        )
        .route(
            "/users/{uuid}/phones/{id}",
            web::put().to(crate::handlers::user_contact::update_user_phone),
        )
        .route(
            "/users/{uuid}/phones/{id}",
            web::delete().to(crate::handlers::user_contact::delete_user_phone),
        )
        .route(
            "/users/{uuid}/addresses",
            web::get().to(crate::handlers::user_contact::list_user_addresses),
        )
        .route(
            "/users/{uuid}/addresses",
            web::post().to(crate::handlers::user_contact::add_user_address),
        )
        .route(
            "/users/{uuid}/addresses/{id}",
            web::put().to(crate::handlers::user_contact::update_user_address),
        )
        .route(
            "/users/{uuid}/addresses/{id}",
            web::delete().to(crate::handlers::user_contact::delete_user_address),
        )
        // Workspace user custom-field schema (read staff, write admin).
        .route(
            "/admin/user-fields",
            web::get().to(crate::handlers::user_contact::get_user_field_schema),
        )
        .route(
            "/admin/user-fields",
            web::put().to(crate::handlers::user_contact::set_user_field_schema),
        )
        // Per-workspace LDAP/directory config (admin-gated in the handlers).
        .route(
            "/ldap/settings",
            web::get().to(crate::handlers::ldap_integration::get_ldap_settings),
        )
        .route(
            "/ldap/sync-history",
            web::get().to(crate::handlers::ldap_integration::get_ldap_sync_history),
        )
        .route(
            "/ldap/settings",
            web::put().to(crate::handlers::ldap_integration::set_ldap_settings),
        )
        .route(
            "/ldap/presets",
            web::get().to(crate::handlers::ldap_integration::get_ldap_presets),
        )
        .route(
            "/ldap/test-connection",
            web::post().to(crate::handlers::ldap_integration::test_ldap_connection),
        )
        .route(
            "/ldap/discover-groups",
            web::get().to(crate::handlers::ldap_integration::discover_ldap_groups),
        )
        .route(
            "/ldap/preview",
            web::post().to(crate::handlers::ldap_integration::preview_ldap),
        )
        .route(
            "/ldap/sync",
            web::post().to(crate::handlers::ldap_integration::run_ldap_sync),
        )
        .route(
            "/users/{uuid}/with-emails",
            web::get().to(crate::handlers::get_user_with_emails),
        )
        .route(
            "/users/{uuid}/profile",
            web::get().to(crate::handlers::users::get_user_profile_bundle),
        )
        .route(
            "/users/{uuid}/auth-identities",
            web::get().to(crate::handlers::get_user_auth_identities_by_uuid),
        )
        .route(
            "/users/{uuid}/auth-identities/{id}",
            web::delete().to(crate::handlers::delete_user_auth_identity_by_uuid),
        )
        .route(
            "/users/{uuid}/resend-invitation",
            web::post().to(crate::handlers::resend_invitation),
        )
        .route(
            "/users/{uuid}/security-info",
            web::get().to(crate::handlers::get_user_security_info),
        )
        .route(
            "/users/{uuid}/reset-password",
            web::post().to(crate::handlers::admin_reset_user_password),
        )
        .route(
            "/users/{uuid}/disable-mfa",
            web::post().to(crate::handlers::admin_disable_user_mfa),
        )
        .route(
            "/users/{uuid}/passkeys/{credential_id}",
            web::delete().to(crate::handlers::admin_delete_user_passkey),
        );
}

/// Result type for invitation sending operations
pub enum SendInvitationResult {
    Success,
    EmailServiceError(String),
    TokenStorageError(String),
    EmailSendError(String),
}

/// Cap the persisted dashboard layout at a kilobyte — we expect a
/// handful of widget ids and booleans, anything approaching 1 KB is a
/// sign of a malformed payload.
const MAX_DASHBOARD_LAYOUT_BYTES: usize = 4 * 1024;

/// Confirmation resends allowed per user per hour. Matches
/// `password_reset::MAX_RESET_REQUESTS_PER_HOUR`: same shape of action, same
/// reason to bound it, so no reason for a different number.
const MAX_VERIFICATION_RESENDS_PER_HOUR: i64 = 3;

/// Validate the shape of a user-supplied `dashboard_layout` JSON blob
/// before it is persisted. Expected form:
///
/// ```json
/// { "widgets": [
///     { "id": "string", "visible": true, "span": 1, "rowSpan": 2, "col": 0, "config": {...} },
///     ...
/// ] }
/// ```
///
/// `config` is an arbitrary per-widget config object — the shape is owned
/// by the widget, not enforced here. The overall byte cap is the
/// backstop against a client shoving megabytes of junk through.
fn validate_dashboard_layout(layout: &serde_json::Value) -> Result<(), &'static str> {
    if serde_json::to_vec(layout)
        .map(|v| v.len())
        .unwrap_or(usize::MAX)
        > MAX_DASHBOARD_LAYOUT_BYTES
    {
        return Err("dashboard_layout exceeds the size limit");
    }
    let obj = layout
        .as_object()
        .ok_or("dashboard_layout must be a JSON object")?;
    let widgets = obj
        .get("widgets")
        .and_then(|w| w.as_array())
        .ok_or("dashboard_layout.widgets must be an array")?;
    for entry in widgets {
        let entry = entry
            .as_object()
            .ok_or("dashboard_layout.widgets[] must be objects")?;
        let id_ok = entry
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty() && s.len() <= 64)
            .unwrap_or(false);
        if !id_ok {
            return Err("dashboard_layout.widgets[].id must be a non-empty string up to 64 chars");
        }
        if !entry
            .get("visible")
            .map(|v| v.is_boolean())
            .unwrap_or(false)
        {
            return Err("dashboard_layout.widgets[].visible must be a boolean");
        }
        // `span` is optional; when present it must be an integer 1-3
        // (the client restricts the UI to these three column spans).
        if let Some(span) = entry.get("span") {
            let span_ok = span.as_i64().map(|n| (1..=3).contains(&n)).unwrap_or(false);
            if !span_ok {
                return Err("dashboard_layout.widgets[].span must be 1, 2, or 3 when present");
            }
        }
        // `rowSpan` is optional; same 1-3 bound as `span` (the corner-
        // resize handle clamps to these row-unit heights).
        if let Some(row_span) = entry.get("rowSpan") {
            let ok = row_span
                .as_i64()
                .map(|n| (1..=3).contains(&n))
                .unwrap_or(false);
            if !ok {
                return Err("dashboard_layout.widgets[].rowSpan must be 1, 2, or 3 when present");
            }
        }
        // `col` is optional; when present it must be an integer 0-2
        // (the anchor column on the client's 3-column lattice; rows
        // are derived client-side and never persisted).
        if let Some(col) = entry.get("col") {
            let ok = col.as_i64().map(|n| (0..=2).contains(&n)).unwrap_or(false);
            if !ok {
                return Err("dashboard_layout.widgets[].col must be 0, 1, or 2 when present");
            }
        }
        // `config` is optional; shape is owned by the widget. Only the
        // outer type is enforced, object or nothing.
        if let Some(config) = entry.get("config") {
            if !config.is_object() {
                return Err("dashboard_layout.widgets[].config must be an object when present");
            }
        }
    }
    Ok(())
}

impl std::fmt::Display for SendInvitationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success => write!(f, "success"),
            Self::EmailServiceError(_) => write!(f, "email_service_error"),
            Self::TokenStorageError(_) => write!(f, "token_storage_error"),
            Self::EmailSendError(_) => write!(f, "email_send_error"),
        }
    }
}

/// Output of [`prepare_invitation`] — ready-to-use pieces that every
/// invitation-email caller needs. Kept private so the two thin public
/// helpers stay the only entry points.
struct PreparedInvitation {
    raw_token: String,
    email_service: crate::utils::email::EmailService,
    branding: crate::utils::email::EmailBranding,
}

/// Mint an invitation token, record it, and load the pieces needed to
/// send the email. Shared prelude for both [`send_user_invitation`] and
/// [`send_guest_ticket_confirmation`] — the two only differ in the
/// metadata stamped on the token and which email template they send.
async fn prepare_invitation(
    conn: &mut DbConnection,
    req: &HttpRequest,
    user_uuid: Uuid,
    metadata: Option<serde_json::Value>,
) -> Result<PreparedInvitation, SendInvitationResult> {
    let invitation_token = crate::utils::reset_tokens::ResetTokenUtils::create_reset_token(
        user_uuid,
        crate::utils::reset_tokens::TokenType::Invitation,
    );

    let ip_address = crate::utils::client_ip::from_http_request(req).map(|ip| ip.to_string());
    let user_agent = req
        .headers()
        .get("User-Agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // The accept page greets with the workspace the person was invited
    // into (and the inviter, when the caller passes one in `metadata`).
    let metadata = {
        let mut m = metadata.unwrap_or_else(|| serde_json::json!({}));
        if let Some(ws) = req
            .extensions()
            .get::<crate::extractors::WorkspaceContext>()
        {
            if let Some(obj) = m.as_object_mut() {
                obj.entry("workspace_name")
                    .or_insert_with(|| serde_json::Value::String(ws.name.clone()));
            }
        }
        Some(m)
    };

    if let Err(e) = repository::reset_tokens::create_reset_token(
        conn,
        &invitation_token.token_hash,
        user_uuid,
        invitation_token.token_type.as_str(),
        ip_address.as_deref(),
        user_agent.as_deref(),
        invitation_token.expires_at,
        metadata,
    ) {
        return Err(SendInvitationResult::TokenStorageError(format!("{e:?}")));
    }

    // Invite into the current workspace, so the link lives on this workspace's
    // canonical origin (custom domain or `<slug>.<NOSDESK_TENANT_DOMAIN>`), else
    // FRONTEND_URL. We deliberately do NOT fall back to the request `Host`
    // header: a forged Host would let an attacker point the invitation link at a
    // domain they control and harvest the token. If neither a canonical origin
    // nor FRONTEND_URL is configured, refuse to send.
    let ws_origin = req
        .extensions()
        .get::<crate::extractors::WorkspaceContext>()
        .and_then(|ws| ws.canonical_origin());
    let Some(base_url) = crate::utils::tenant_origin::email_link_base(ws_origin) else {
        return Err(SendInvitationResult::EmailServiceError(
            "no canonical origin and FRONTEND_URL is unset; set FRONTEND_URL so invitation \
             links can't be forged via the Host header"
                .to_string(),
        ));
    };

    let email_service = crate::utils::email::EmailService::from_env()
        .map_err(|e| SendInvitationResult::EmailServiceError(format!("{e:?}")))?;
    let branding = get_email_branding(conn, &base_url);

    Ok(PreparedInvitation {
        raw_token: invitation_token.raw_token,
        email_service,
        branding,
    })
}

/// Create and send a generic (admin-initiated) invitation email.
/// Used by `create_user` and `resend_invitation`, and reused by the
/// guest-ticket flow when the legacy no-verification path is active.
pub async fn send_user_invitation(
    conn: &mut DbConnection,
    req: &HttpRequest,
    user_uuid: Uuid,
    user_email: &str,
    user_name: &str,
    admin_name: &str,
) -> SendInvitationResult {
    let prep = match prepare_invitation(
        conn,
        req,
        user_uuid,
        Some(serde_json::json!({ "invited_by": admin_name })),
    )
    .await
    {
        Ok(p) => p,
        Err(result) => return result,
    };

    // Enqueue rather than synchronous send. The worker handles retry
    // with backoff and respects the suppression list, so an SMTP
    // hiccup mid-onboarding doesn't lose the invitation.
    let locale = crate::repository::user_locale::resolve_effective_locale(conn, user_uuid);
    match crate::services::transactional_email::enqueue_invitation(
        conn,
        &prep.email_service,
        &prep.branding,
        user_email,
        user_name,
        &prep.raw_token,
        admin_name,
        &locale,
    ) {
        Ok(_) => SendInvitationResult::Success,
        Err(e) => SendInvitationResult::EmailSendError(format!("{e:?}")),
    }
}

/// Create and send a guest-ticket-confirmation email — same invitation
/// token/flow as [`send_user_invitation`] but with submission-themed copy
/// and a metadata tag that the accept handler can inspect.
pub async fn send_guest_ticket_confirmation(
    conn: &mut DbConnection,
    req: &HttpRequest,
    user_uuid: Uuid,
    user_email: &str,
    user_name: &str,
) -> SendInvitationResult {
    let prep = match prepare_invitation(
        conn,
        req,
        user_uuid,
        Some(serde_json::json!({ "source": "guest_ticket_submission" })),
    )
    .await
    {
        Ok(p) => p,
        Err(result) => return result,
    };

    match prep
        .email_service
        .send_guest_ticket_confirmation_email(
            user_email,
            user_name,
            &prep.raw_token,
            &prep.branding,
        )
        .await
    {
        Ok(_) => SendInvitationResult::Success,
        Err(e) => SendInvitationResult::EmailSendError(format!("{e:?}")),
    }
}

// Pagination query parameters
#[derive(Deserialize)]
pub struct PaginationParams {
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
    #[serde(rename = "sortField")]
    sort_field: Option<String>,
    #[serde(rename = "sortDirection")]
    sort_direction: Option<String>,
    search: Option<String>,
    role: Option<String>,
    /// People population: `"team"` (staff) or `"requesters"` (end-users).
    /// Absent shows the combined directory.
    population: Option<String>,
    /// Filter on soft-delete state. `"active"` (default) excludes
    /// rows with `deleted_at` set; `"deleted"` selects only those
    /// rows; `"all"` returns both. The admin "Deleted users" tab
    /// switches the chip to `"deleted"`.
    deleted: Option<String>,
}

// Paginated response
#[derive(Serialize)]
pub struct PaginatedResponse<T> {
    data: Vec<T>,
    total: i64,
    page: i64,
    #[serde(rename = "pageSize")]
    page_size: i64,
    #[serde(rename = "totalPages")]
    total_pages: i64,
}

// User handlers
pub async fn get_users(
    pool: web::Data<crate::db::Pool>,
    ws: WorkspaceContext,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&pool)?;
    // Pin the resolved workspace so the users read and per-row workspace_role
    // lookup are visible under RLS (both tables are workspace-isolated).
    helpers::pin_workspace(&mut conn, ws.workspace_id);

    match repository::get_users(&mut conn) {
        Ok(users) => {
            // Convert users to UserResponse with emails (batch fetch for efficiency)
            let user_responses = repository::user_helpers::get_users_with_primary_emails(
                users,
                &mut conn,
                ws.workspace_id,
            );
            Ok(HttpResponse::Ok().json(user_responses))
        }
        Err(e) => {
            error!(error = ?e, "Error fetching users");
            Err(ApiError::Internal("Failed to fetch users".into()))
        }
    }
}

// Get paginated users
pub async fn get_paginated_users(
    pool: web::Data<crate::db::Pool>,
    query: web::Query<PaginationParams>,
    req: HttpRequest,
    ws: WorkspaceContext,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&pool)?;
    // Pin the resolved workspace so the users read and per-row workspace_role
    // lookup are visible under RLS (both tables are workspace-isolated).
    helpers::pin_workspace(&mut conn, ws.workspace_id);

    // Extract and validate pagination parameters
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(25).clamp(1, 100);

    // Validate sort_field against allowed columns
    // Gate before the repository sees it. Anything not listed here is
    // dropped to `None` and the repository falls back to name-ascending
    // — so a field with a sort arm but no entry here silently does
    // nothing. The two `*_count` fields sort via correlated subqueries
    // (see `get_paginated_users`), not real columns.
    let allowed_sort_fields = [
        "name",
        "first_name",
        "last_name",
        "email",
        "role",
        "created_at",
        "updated_at",
        "open_ticket_count",
        "device_count",
    ];
    let sort_field = query.sort_field.as_ref().and_then(|f| {
        let f_lower = f.to_lowercase();
        if allowed_sort_fields.contains(&f_lower.as_str()) {
            Some(f_lower)
        } else {
            None
        }
    });

    // Validate sort_direction
    let sort_direction = query.sort_direction.as_ref().and_then(|d| {
        let d_lower = d.to_lowercase();
        if d_lower == "asc" || d_lower == "desc" {
            Some(d_lower)
        } else {
            None
        }
    });

    // Limit search string length to prevent DoS (char-safe; len() alone is UTF-8 bytes)
    let search = query
        .search
        .as_ref()
        .map(|s| crate::utils::utf8_trunc::char_prefix(s, 100));

    // Validate role filter
    let allowed_roles = ["admin", "agent", "user"];
    let role = query.role.as_ref().and_then(|r| {
        let r_lower = r.to_lowercase();
        if allowed_roles.contains(&r_lower.as_str()) {
            Some(r_lower)
        } else {
            None
        }
    });

    // Soft-deleted users (and the "all" view) are a platform-admin-only
    // recovery surface, matching who can restore/purge them. A non-admin
    // caller is forced back to the active set regardless of the requested
    // filter, so deleted tombstones (and the PII they retain until purge)
    // never leak to ordinary staff who can reach this directory endpoint.
    let requested_deleted = repository::users::DeletedFilter::from_query(query.deleted.as_deref());
    let is_admin = crate::utils::rbac::require_auth(&req)
        .map(|claims| is_platform_admin(&claims))
        .unwrap_or(false);
    let deleted = if is_admin {
        requested_deleted
    } else {
        repository::users::DeletedFilter::Active
    };

    // People population filter (Team = staff, Requesters = end-users); the
    // combined directory when absent.
    let population = repository::users::Population::from_query(query.population.as_deref());

    match repository::get_paginated_users(
        &mut conn,
        page,
        page_size,
        sort_field,
        sort_direction,
        search,
        role,
        population,
        deleted,
        ws.workspace_id,
    ) {
        Ok((users, total)) => {
            // Calculate total pages
            let total_pages = (total as f64 / page_size as f64).ceil() as i64;

            // Convert users to UserResponse with emails (batch fetch for efficiency)
            let mut user_responses = repository::user_helpers::get_users_with_primary_emails(
                users,
                &mut conn,
                ws.workspace_id,
            );

            // Enrich with ticket and device counts
            let user_uuids: Vec<Uuid> = user_responses.iter().map(|u| u.uuid).collect();
            if !user_uuids.is_empty() {
                let ticket_counts = get_open_ticket_counts_batch(&user_uuids, &mut conn);
                let device_counts = get_device_counts_batch(&user_uuids, &mut conn);
                for user in &mut user_responses {
                    user.open_ticket_count = Some(*ticket_counts.get(&user.uuid).unwrap_or(&0));
                    user.device_count = Some(*device_counts.get(&user.uuid).unwrap_or(&0));
                }
            }

            // Create paginated response
            let response = PaginatedResponse {
                data: user_responses,
                total,
                page,
                page_size,
                total_pages,
            };

            Ok(HttpResponse::Ok().json(response))
        }
        Err(e) => {
            error!(error = ?e, "Error fetching paginated users");
            Err(ApiError::Internal("Failed to get paginated users".into()))
        }
    }
}

/// Batch count of open (non-closed) tickets assigned to each user
fn get_open_ticket_counts_batch(
    user_uuids: &[Uuid],
    conn: &mut DbConnection,
) -> HashMap<Uuid, i64> {
    use crate::models::WorkflowStateCategory;
    use crate::schema::{tickets, workflow_states};

    // "Open" === any non-terminal workflow state category. Join
    // workflow_states and filter on the four non-terminal categories.
    let open_categories = vec![
        WorkflowStateCategory::Triage,
        WorkflowStateCategory::Backlog,
        WorkflowStateCategory::Active,
        WorkflowStateCategory::InReview,
    ];
    let results: Vec<(Uuid, i64)> = tickets::table
        .inner_join(workflow_states::table)
        .filter(tickets::assignee_uuid.eq_any(user_uuids))
        .filter(workflow_states::category.eq_any(open_categories))
        .group_by(tickets::assignee_uuid)
        .select((
            tickets::assignee_uuid.assume_not_null(),
            diesel::dsl::count_star(),
        ))
        .load::<(Uuid, i64)>(conn)
        .unwrap_or_default();

    results.into_iter().collect()
}

/// Batch count of devices assigned to each user
fn get_device_counts_batch(user_uuids: &[Uuid], conn: &mut DbConnection) -> HashMap<Uuid, i64> {
    use crate::schema::assets;

    let results: Vec<(Uuid, i64)> = assets::table
        .filter(assets::primary_user_uuid.eq_any(user_uuids))
        .group_by(assets::primary_user_uuid)
        .select((
            assets::primary_user_uuid.assume_not_null(),
            diesel::dsl::count_star(),
        ))
        .load::<(Uuid, i64)>(conn)
        .unwrap_or_default();

    results.into_iter().collect()
}

pub async fn get_user_by_uuid(
    uuid_path: web::Path<String>,
    pool: web::Data<crate::db::Pool>,
    ws: WorkspaceContext,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let uuid_str = uuid_path.into_inner();

    // Parse the UUID string into a proper UUID type
    let user_uuid_parsed = match utils::parse_uuid(&uuid_str) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID format".into())),
    };

    let mut conn = helpers::db_conn(&pool)?;
    // Pin the request's workspace so the returned user's workspace_role
    // resolves under RLS (workspace_members is workspace-isolated).
    helpers::pin_request_workspace(&req, &mut conn);

    // Resolve through the membership join, not by uuid alone. `users` has no
    // RLS (an account spans workspaces, so there is no column to key on), and
    // the pin above only scopes the RLS-bearing tables joined alongside it, so
    // a bare `find(uuid)` here read any user in the deployment — name, avatar
    // and primary email — for any authenticated member of any workspace.
    match repository::directory::find_member(&mut conn, ws.workspace_id, user_uuid_parsed) {
        Ok(Some(user)) => {
            // Use helper function to fetch primary email from user_emails table
            let user_response =
                repository::user_helpers::get_user_with_primary_email(user, &mut conn);
            Ok(HttpResponse::Ok().json(user_response))
        }
        // A stranger is indistinguishable from a missing row, so this does not
        // become an oracle for which uuids exist in other workspaces.
        Ok(None) => Err(ApiError::NotFoundMsg("User not found".into())),
        Err(_) => Err(ApiError::NotFoundMsg("User not found".into())),
    }
}

// Batch users request
#[derive(Deserialize)]
pub struct BatchUsersRequest {
    uuids: Vec<String>,
}

pub async fn get_users_batch(
    batch_request: web::Json<BatchUsersRequest>,
    pool: web::Data<crate::db::Pool>,
    ws: WorkspaceContext,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&pool)?;
    // Pin the resolved workspace so the users read and per-row workspace_role
    // lookup are visible under RLS (both tables are workspace-isolated).
    helpers::pin_workspace(&mut conn, ws.workspace_id);

    // Validate UUIDs and remove duplicates
    let mut valid_uuids = HashSet::new();
    for uuid_str in &batch_request.uuids {
        if let Ok(uuid) = Uuid::parse_str(uuid_str) {
            valid_uuids.insert(uuid);
        }
    }

    if valid_uuids.is_empty() {
        return Err(ApiError::BadRequest("No valid UUIDs provided".into()));
    }

    // Convert to Vec for the repository function
    let uuids_vec: Vec<Uuid> = valid_uuids.into_iter().collect();

    // Same membership join as the singular lookup: a batch endpoint must not
    // become a cheaper oracle for the identities the singular one now hides.
    // Unknown uuids are simply absent from the response.
    match repository::directory::find_members(&mut conn, ws.workspace_id, &uuids_vec) {
        Ok(users) => {
            // Convert users to UserResponse with emails (batch fetch for efficiency)
            let user_responses = repository::user_helpers::get_users_with_primary_emails(
                users,
                &mut conn,
                ws.workspace_id,
            );
            Ok(HttpResponse::Ok().json(user_responses))
        }
        Err(e) => {
            error!(error = ?e, "Error fetching users batch");
            Err(ApiError::Internal("Failed to get users".into()))
        }
    }
}

// API request model for user creation (includes email which goes in user_emails table)
#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    /// Optional UUID - if not provided, a new UUIDv7 will be generated
    uuid: Option<Uuid>,
    name: String,
    email: String,
    /// Requested role string ("admin" / "technician" / "user" /
    /// "audit_reviewer"), mapped to the platform + workspace role
    /// split via `utils::parse_roles`.
    role: String,
    pronouns: Option<String>,
    avatar_url: Option<String>,
    banner_url: Option<String>,
    avatar_thumb: Option<String>,
    microsoft_uuid: Option<Uuid>,
    /// Optional password - if provided, sets the password directly.
    /// If not provided and SMTP is configured, sends an invitation email.
    /// If not provided and SMTP is not configured, returns an error.
    password: Option<String>,
    /// Whether to send an invitation email (only used when SMTP is configured and no password provided)
    send_invitation: Option<bool>,
}

pub async fn create_user(
    db_pool: web::Data<crate::db::Pool>,
    search_service: web::Data<Arc<SearchService>>,
    user_data: web::Json<CreateUserRequest>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    // The intended role decides the gate (roles map to the platform+workspace
    // split via parse_roles, so we resolve it up front).
    //
    // A plain requester (end-user) is a tenant-local `member` with platform
    // role `user`: not a billed seat and with no control-plane identity. Any
    // agent-or-above may add one directly, in hosted and self-hosted alike (an
    // agent logging a phone-in ticket). Staff and platform roles (agent/admin/
    // audit_reviewer) are seats: workspace-admin, and without this gate any
    // authenticated user could POST role:"admin" and mint a privileged account
    // (security-audit-2026-06). In hosted they are control-plane owned, so hand
    // off there rather than mint a local account that can never sign in.
    let (platform_role, workspace_role) = match utils::parse_roles(&user_data.role) {
        Ok(roles) => roles,
        Err(e) => return Ok(e.into()),
    };
    let is_requester = platform_role == crate::models::PlatformRole::User
        && workspace_role == crate::models::WorkspaceRole::Member;

    let actor_claims = if is_requester {
        crate::utils::rbac::require_workspace_role(&req, crate::models::WorkspaceRole::Agent)?
    } else {
        let claims =
            crate::utils::rbac::require_workspace_role(&req, crate::models::WorkspaceRole::Admin)?;
        if !crate::middleware::workspace_context::local_credentials_permitted() {
            // Same boundary + code as the other staff-mutation refusals.
            return Ok(errors::externally_managed());
        }
        claims
    };

    let mut conn = helpers::db_conn(&db_pool)?;
    // Pin the request's workspace at the session level so the new user's
    // workspace_role resolves under RLS when the response is built after
    // the creation transaction (with_actor_context's SET LOCAL reverts to
    // this value on commit).
    helpers::pin_request_workspace(&req, &mut conn);

    // Resolve the inviter's display name for the invitation email.
    let admin_name = uuid::Uuid::parse_str(&actor_claims.sub)
        .ok()
        .and_then(|uuid| repository::get_user_by_uuid(&uuid, &mut conn).ok())
        .map(|u| u.name)
        .unwrap_or_else(|| "An administrator".to_string());

    // Check email configuration. EmailService::from_env builds the SMTP
    // transport and reports is_configured for it.
    let smtp_configured = crate::utils::email::EmailService::from_env()
        .map(|s| s.is_configured())
        .unwrap_or(false);

    // Comprehensive input validation using our validation utilities
    let mut validation_errors = Vec::new();

    // Validate name
    let trimmed_name = user_data.name.trim();
    if trimmed_name.is_empty() {
        validation_errors.push("name: Name is required".to_string());
    } else if trimmed_name.len() > 255 {
        validation_errors.push("name: Name must be less than 255 characters".to_string());
    }

    // Validate email
    if user_data.email.is_empty() {
        validation_errors.push("email: Email is required".to_string());
    } else if !user_data.email.contains('@') {
        validation_errors.push("email: Invalid email format".to_string());
    }

    // Validate password if provided
    if let Some(ref password) = user_data.password {
        if password.len() < 8 {
            validation_errors.push("password: Password must be at least 8 characters".to_string());
        } else if password.len() > 128 {
            validation_errors
                .push("password: Password must be less than 128 characters".to_string());
        }
    }

    // If no password provided and SMTP not configured, require password
    if user_data.password.is_none() && !smtp_configured {
        validation_errors
            .push("password: Password is required when email is not configured".to_string());
    }

    if !validation_errors.is_empty() {
        return Ok(errors::validation_failed(validation_errors));
    }

    // Validate optional fields
    if let Some(ref pronouns) = user_data.pronouns {
        if pronouns.len() > 50 {
            validation_errors
                .push("pronouns: Pronouns must be less than 50 characters".to_string());
        }
    }

    if let Some(ref avatar_url) = user_data.avatar_url {
        if avatar_url.len() > 500 {
            validation_errors.push("avatar_url: URL must be less than 500 characters".to_string());
        }
    }

    if let Some(ref banner_url) = user_data.banner_url {
        if banner_url.len() > 500 {
            validation_errors.push("banner_url: URL must be less than 500 characters".to_string());
        }
    }

    if let Some(ref avatar_thumb) = user_data.avatar_thumb {
        if avatar_thumb.len() > 500 {
            validation_errors
                .push("avatar_thumb: URL must be less than 500 characters".to_string());
        }
    }

    // Check if user with this email already exists
    if repository::get_user_by_email(&user_data.email, &mut conn).is_ok() {
        return Err(ApiError::BadRequest(
            "User with this email already exists".into(),
        ));
    }

    // Use provided UUID or generate a new UUIDv7
    let user_uuid = user_data.uuid.unwrap_or_else(uuid::Uuid::now_v7);

    // Create new user with normalized data using builder
    let (normalized_name, normalized_email) =
        utils::normalization::normalize_user_data(&user_data.name, &user_data.email);
    let (new_user, email) =
        utils::NewUserBuilder::new(normalized_name.clone(), normalized_email, platform_role)
            .with_uuid(user_uuid)
            .with_pronouns(user_data.pronouns.as_ref().map(|p| p.trim().to_string()))
            .with_avatar(
                user_data.avatar_url.as_ref().map(|u| u.trim().to_string()),
                user_data
                    .avatar_thumb
                    .as_ref()
                    .map(|u| u.trim().to_string()),
            )
            .with_banner(user_data.banner_url.as_ref().map(|u| u.trim().to_string()))
            .with_microsoft_uuid(user_data.microsoft_uuid)
            .build_with_email();

    // `create_user_with_email` writes the audited `users` table and
    // emits a sync event, so it needs `app.workspace_id` pinned for the
    // NDX01 trigger to accept the write. Wrap just this call in actor
    // context rather than migrating the whole handler to TenantConn:
    // the surrounding flow includes an async invitation send that holds
    // the connection across an `.await` (which TenantConn's sync `run`
    // closure can't express), and every other op here touches
    // non-audited platform tables (user_auth_identities, reset_tokens).
    //
    // Email starts as unverified - will be verified when the user
    // accepts the invitation or verifies the email.
    let create_actor = helpers::actor_for(&req, "create_user");
    let create_result = crate::sync::session::with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &create_actor,
        |c| {
            repository::user_helpers::create_user_with_email(
                new_user,
                workspace_role,
                email.clone(),
                false,
                Some("manual".to_string()),
                c,
                Some(search_service.get_ref()),
                // Product surface; staff creation in hosted is already refused
                // earlier in this handler, so this never gates in practice.
                repository::workspaces::SeatWriteAuthority::Product,
            )
            .and_then(|o| o.into_created())
        },
    );
    match create_result {
        Ok((user, _email_entry)) => {
            use crate::models::NewUserAuthIdentity;
            use bcrypt::hash;

            // Determine how to handle authentication setup
            let (password_hash, invitation_sent) = if let Some(ref password) = user_data.password {
                // Password provided - hash it directly
                let hash = match hash(password, DEFAULT_COST) {
                    Ok(h) => h,
                    Err(e) => {
                        error!(error = ?e, "Error hashing password");
                        return Err(ApiError::Internal("Error setting password".into()));
                    }
                };
                (Some(hash), false)
            } else if smtp_configured && user_data.send_invitation.unwrap_or(true) {
                // No password, SMTP configured - send invitation email
                match send_user_invitation(
                    &mut conn,
                    &req,
                    user.uuid,
                    &email,
                    &normalized_name,
                    &admin_name,
                )
                .await
                {
                    SendInvitationResult::Success => {
                        // Invitation sent successfully
                    }
                    SendInvitationResult::TokenStorageError(e) => {
                        error!(error = %e, "Error storing invitation token");
                        return Err(ApiError::Internal("Error creating invitation".into()));
                    }
                    SendInvitationResult::EmailServiceError(e) => {
                        error!(error = %e, "Error initializing email service");
                        return Err(ApiError::Internal("Error sending invitation email".into()));
                    }
                    SendInvitationResult::EmailSendError(e) => {
                        error!(error = %e, "Error sending invitation email");
                        // Don't fail - user was created, just couldn't send email
                        warn!("User created but invitation email failed to send");
                    }
                }

                (None, true) // No password hash - user will set via invitation
            } else {
                // No password and no SMTP - this should have been caught in validation
                return Err(ApiError::BadRequest(
                    "Password is required when email is not configured".into(),
                ));
            };

            debug!(user_uuid = %user.uuid, "Created user");

            // Create local auth identity (with or without password)
            let new_identity = NewUserAuthIdentity {
                user_uuid: user.uuid,
                provider_type: "local".to_string(),
                external_id: utils::uuid_to_string(&user.uuid),
                email: Some(email.clone()),
                metadata: None,
                password_hash,
                workspace_id: None,
            };

            match repository::user_auth_identities::create_local_identity(new_identity, &mut conn) {
                Ok(_) => {
                    if invitation_sent {
                        info!(user_name = %user.name, "New user created (invitation email sent)");
                    } else {
                        info!(user_name = %user.name, "New user created (password set)");
                    }
                    let response = repository::user_helpers::get_user_with_primary_email(
                        user.clone(),
                        &mut conn,
                    );

                    // Search index update is fired by the
                    // UserCreatedObserver inside create_user_with_email
                    // above. The new user reaches clients through the
                    // sync pool (the repository write emits
                    // `user.created`), so no discrete SSE broadcast.

                    // Add invitation_sent flag to response
                    if let serde_json::Value::Object(mut map) =
                        serde_json::to_value(&response).unwrap_or_default()
                    {
                        map.insert(
                            "invitation_sent".to_string(),
                            serde_json::Value::Bool(invitation_sent),
                        );
                        return Ok(HttpResponse::Created().json(map));
                    }
                    Ok(HttpResponse::Created().json(response))
                }
                Err(e) => {
                    error!(error = ?e, "Error creating auth identity");
                    // If identity creation fails, still return the user (with primary email)
                    let user_response =
                        repository::user_helpers::get_user_with_primary_email(user, &mut conn);
                    Ok(HttpResponse::Created().json(user_response))
                }
            }
        }
        Err(e) => {
            error!(error = ?e, "Error creating user");

            // Provide more specific error messages for common issues
            let error_message =
                if format!("{e:?}").contains("duplicate") || format!("{e:?}").contains("unique") {
                    if format!("{e:?}").contains("email") {
                        "Email address already exists in the system"
                    } else if format!("{e:?}").contains("uuid") {
                        "UUID already exists in the system"
                    } else {
                        "Duplicate entry detected"
                    }
                } else {
                    "Error creating user"
                };

            Ok(errors::internal(error_message))
        }
    }
}

/// Common prelude for the three single-target admin endpoints
/// (`delete_user`, `restore_user`, `purge_user_now`): pull
/// claims, require admin role, parse the path UUID, and fetch
/// the target row. Returns `(claims, uuid, target_user)` on the
/// happy path or the error to short-circuit on.
fn require_admin_target(
    req: &HttpRequest,
    conn: &mut DbConnection,
    raw_uuid: &str,
) -> Result<(crate::models::Claims, Uuid, crate::models::User), ApiError> {
    let claims = req
        .extensions()
        .get::<crate::models::Claims>()
        .cloned()
        .ok_or_else(|| ApiError::Unauthorized("Authentication required".into()))?;
    if !is_platform_admin(&claims) {
        return Err(ApiError::Forbidden(
            "Only administrators can perform this action".into(),
        ));
    }
    let user_uuid = utils::parse_uuid(raw_uuid)
        .map_err(|_| ApiError::BadRequest("Invalid UUID format".into()))?;
    let target = repository::get_user_by_uuid(&user_uuid, conn)
        .map_err(|_| ApiError::NotFoundMsg("User not found".into()))?;
    Ok((claims, user_uuid, target))
}

/// Soft-delete a user. Stamps `users.deleted_at`, emits a sync
/// event so frontends drop the row from active surfaces, and
/// schedules the destructive cascade via the
/// `user_purge_worker` after the configured retention window.
/// Restorable via `POST /admin/users/{uuid}/restore` until purge.
pub async fn delete_user(
    uuid: web::Path<String>,
    pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&pool)?;
    // Pin the request's workspace so the admin-protection guard below
    // (`user_is_admin` reads RLS-isolated workspace_members) resolves the
    // target's role. On an unpinned conn the role reads as None and the guard
    // fails OPEN, letting an admin account be deleted.
    helpers::pin_request_workspace(&req, &mut conn);

    let (claims, user_uuid_parsed, target_user) =
        require_admin_target(&req, &mut conn, uuid.as_str())?;

    if claims.sub == uuid.as_str() {
        return Err(ApiError::BadRequest(
            "You cannot delete your own account while logged in".into(),
        ));
    }
    if crate::repository::user_helpers::user_is_admin(&mut conn, &target_user) {
        return Err(ApiError::BadRequest(
            "Administrator accounts cannot be deleted for security reasons".into(),
        ));
    }
    let actor = helpers::actor_for(&req, "users_admin");
    // In hosted, a staff seat is control-plane-owned in ANY workspace (the CP
    // owns the account lifecycle). Refuse a local delete and hand off.
    if helpers::target_is_externally_managed_staff(&mut conn, &actor, target_user.uuid) {
        return Ok(errors::externally_managed());
    }
    if target_user.deleted_at.is_some() {
        return Err(ApiError::Conflict("User is already soft-deleted".into()));
    }

    let soft_deleted = match crate::sync::session::with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &actor,
        |conn| repository::users::soft_delete_user(&user_uuid_parsed, conn),
    ) {
        Ok(u) => u,
        Err(e) => {
            error!(
                user = %target_user.name,
                user_uuid = %target_user.uuid,
                error = ?e,
                "Failed to soft-delete user"
            );
            return Err(ApiError::Internal("Failed to delete user".into()));
        }
    };

    let purge_at = soft_deleted
        .deleted_at
        .map(|d| d + repository::users::purge_grace_window());

    info!(
        user = %target_user.name,
        user_uuid = %target_user.uuid,
        ?purge_at,
        "User soft-deleted"
    );

    Ok(HttpResponse::Ok().json(json!({
        "uuid": soft_deleted.uuid,
        "deleted_at": soft_deleted.deleted_at,
        "purge_at": purge_at,
    })))
}

/// Restore a soft-deleted user. Clears `deleted_at`, emits
/// `user.restored`, and the active-user surfaces start showing
/// them again on the next sync delta. Cached sessions stay
/// revoked (the auth gate already invalidated them); the user
/// must re-authenticate to act.
pub async fn restore_user(
    uuid: web::Path<String>,
    pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&pool)?;

    let (_claims, user_uuid_parsed, target) = require_admin_target(&req, &mut conn, uuid.as_str())?;
    if target.deleted_at.is_none() {
        return Err(ApiError::Conflict("User is not soft-deleted".into()));
    }
    let actor = helpers::actor_for(&req, "users_admin");
    // In hosted, re-activating a control-plane-owned staff seat (in any
    // workspace) locally would diverge from the projection; hand off.
    if helpers::target_is_externally_managed_staff(&mut conn, &actor, target.uuid) {
        return Ok(errors::externally_managed());
    }

    let restored = match crate::sync::session::with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &actor,
        |conn| repository::users::restore_user(&user_uuid_parsed, conn),
    ) {
        Ok(u) => u,
        Err(e) => {
            error!(user_uuid = %user_uuid_parsed, error = ?e, "Failed to restore user");
            return Err(ApiError::Internal("Failed to restore user".into()));
        }
    };

    info!(user_uuid = %restored.uuid, name = %restored.name, "User restored");

    Ok(HttpResponse::Ok().json(json!({
        "uuid": restored.uuid,
        "deleted_at": restored.deleted_at,
    })))
}

/// Permanently delete a soft-deleted user. The "right to be
/// forgotten" / GDPR erasure path: when a customer asks for
/// their personal data to be removed immediately rather than at
/// the end of the retention window, an admin opens the deleted
/// users list and clicks Permanently Delete. The user must
/// already be soft-deleted; this endpoint never hard-deletes an
/// active row directly. Bypassing the retention worker means we
/// also skip the worker's batching, but the row count per call
/// is one so the destructive cascade runs synchronously.
pub async fn purge_user_now(
    uuid: web::Path<String>,
    pool: web::Data<crate::db::Pool>,
    search_service: web::Data<Arc<SearchService>>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&pool)?;
    // Pin the request's workspace so the admin-protection guard below
    // (`user_is_admin` reads RLS-isolated workspace_members) resolves the
    // target's role; otherwise it reads None and the guard fails OPEN.
    helpers::pin_request_workspace(&req, &mut conn);

    let (claims, user_uuid_parsed, target) = require_admin_target(&req, &mut conn, uuid.as_str())?;
    if target.deleted_at.is_none() {
        return Err(ApiError::Conflict("User must be soft-deleted before permanent deletion. Use DELETE /admin/users/{uuid} first.".into()));
    }
    if claims.sub == uuid.as_str() {
        return Err(ApiError::BadRequest(
            "You cannot permanently delete your own account".into(),
        ));
    }
    if crate::repository::user_helpers::user_is_admin(&mut conn, &target) {
        return Err(ApiError::BadRequest(
            "Administrator accounts cannot be deleted for security reasons".into(),
        ));
    }
    let actor = helpers::actor_for(&req, "users_admin");
    // In hosted, a staff seat is control-plane-owned in ANY workspace; erasure
    // of a projected staff account is a control-plane concern. Refuse.
    if helpers::target_is_externally_managed_staff(&mut conn, &actor, target.uuid) {
        return Ok(errors::externally_managed());
    }

    // Purge cascades across every workspace the user belongs to
    // (workspace_members is many-to-many). A workspace-pinned
    // actor would only match the admin's current workspace and
    // leave orphans elsewhere, breaking the next purge with an
    // FK violation. with_actor_bypass_context (nosdesk_admin,
    // BYPASSRLS) is the correct shape — same as the scheduler-
    // driven purge_soft_deleted_users path.
    let result = crate::sync::session::with_actor_bypass_context::<_, diesel::result::Error>(
        &mut conn,
        &actor,
        |conn| {
            repository::users::purge_user(&user_uuid_parsed, conn, Some(search_service.get_ref()))
        },
    );
    match result {
        Ok(count) if count > 0 => {
            info!(
                user = %target.name,
                user_uuid = %target.uuid,
                "User permanently deleted by admin (skipped retention window)"
            );
            Ok(HttpResponse::NoContent().finish())
        }
        Ok(_) => Err(ApiError::NotFoundMsg("User not found".into())),
        Err(e) => {
            error!(user_uuid = %user_uuid_parsed, error = ?e, "Failed to permanently delete user");
            Err(ApiError::Internal(
                "Failed to permanently delete user".into(),
            ))
        }
    }
}

// Get user's authentication identities
pub async fn get_user_auth_identities(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    // Get database connection
    let mut conn = helpers::db_conn(&db_pool)?;

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return Err(ApiError::Unauthorized("Authentication required".into()));
        }
    };

    // Get the user ID
    let user_uuid_parsed = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID in token".into())),
    };

    let user = match repository::get_user_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(e) => {
            error!(error = ?e, "Error getting user by UUID");
            return Err(ApiError::NotFoundMsg("User not found".into()));
        }
    };

    // Get auth identities for the user
    match repository::user_auth_identities::get_user_identities_display(&user.uuid, &mut conn) {
        Ok(identities) => Ok(HttpResponse::Ok().json(identities)),
        Err(e) => {
            error!(error = ?e, "Error fetching auth identities");
            Err(ApiError::Internal(
                "Failed to retrieve auth identities".into(),
            ))
        }
    }
}

// Get user's authentication identities by UUID
pub async fn get_user_auth_identities_by_uuid(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<String>, // User UUID
) -> Result<HttpResponse, ApiError> {
    // Get database connection
    let mut conn = helpers::db_conn(&db_pool)?;

    let user_uuid = path.into_inner();

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return Err(ApiError::Unauthorized("Authentication required".into()));
        }
    };

    // Ensure the user is authorized (either accessing their own identities or is an admin)
    if claims.sub != user_uuid && !is_platform_admin(&claims) {
        warn!(requesting_user = %claims.sub, target_user = %user_uuid, "Authorization failed: user tried to access identities of another user");
        return Err(ApiError::Forbidden(
            "Not authorized to access this resource".into(),
        ));
    }

    // Get auth identities for the user by UUID
    let user_uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID format".into())),
    };

    match repository::user_auth_identities::get_user_identities_display(
        &user_uuid_parsed,
        &mut conn,
    ) {
        Ok(identities) => Ok(HttpResponse::Ok().json(identities)),
        Err(e) => {
            error!(user_uuid = %user_uuid, error = ?e, "Error fetching auth identities for UUID");
            Err(ApiError::NotFoundMsg(
                "User not found or no auth identities".into(),
            ))
        }
    }
}

// Delete a user authentication identity
pub async fn delete_user_auth_identity(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<i32>, // Auth identity ID
) -> Result<HttpResponse, ApiError> {
    let identity_id = path.into_inner();
    let mut conn = helpers::db_conn(&db_pool)?;

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return Err(ApiError::Unauthorized("Authentication required".into())),
    };

    // Get the user ID
    let user_uuid_parsed = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID in token".into())),
    };

    let user = match repository::get_user_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(e) => {
            error!(error = ?e, "Error getting user by UUID");
            return Err(ApiError::Internal("Failed to get user data".into()));
        }
    };

    // Ensure the user has at least one other auth method before deleting
    // (to prevent locking themselves out)
    let identities =
        match repository::user_auth_identities::get_user_identities(&user.uuid, &mut conn) {
            Ok(identities) => identities,
            Err(e) => {
                error!(error = ?e, "Error getting user auth identities");
                return Err(ApiError::Internal(
                    "Failed to get authentication identities".into(),
                ));
            }
        };

    if identities.len() <= 1 {
        return Err(ApiError::BadRequest(
            "Cannot delete the only authentication method. Add another method first.".into(),
        ));
    }

    // Delete the identity
    match repository::user_auth_identities::delete_identity(identity_id, &user.uuid, &mut conn) {
        Ok(count) => {
            if count == 0 {
                Err(ApiError::NotFoundMsg(
                    "Authentication identity not found or doesn't belong to you".into(),
                ))
            } else {
                Ok(HttpResponse::Ok().json(json!({
                    "status": "success",
                    "message": "Authentication identity deleted successfully"
                })))
            }
        }
        Err(e) => {
            error!(error = ?e, "Error deleting user auth identity");
            Err(ApiError::Internal(
                "Failed to delete authentication identity".into(),
            ))
        }
    }
}

// Delete a user authentication identity by UUID
pub async fn delete_user_auth_identity_by_uuid(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<(String, i32)>, // (User UUID, Auth identity ID)
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, identity_id) = path.into_inner();
    let mut conn = helpers::db_conn(&db_pool)?;

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return Err(ApiError::Unauthorized("Authentication required".into())),
    };

    // Ensure the user is authorized (either accessing their own identities or is an admin)
    if claims.sub != user_uuid && !is_platform_admin(&claims) {
        return Err(ApiError::Forbidden(
            "Not authorized to access this resource".into(),
        ));
    }

    // Ensure the user has at least one other auth method before deleting
    // (to prevent locking themselves out)
    let user_uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID format".into())),
    };

    let identities =
        match repository::user_auth_identities::get_user_identities(&user_uuid_parsed, &mut conn) {
            Ok(identities) => identities,
            Err(e) => {
                error!(error = ?e, "Error getting user auth identities");
                return Err(ApiError::NotFoundMsg("User not found".into()));
            }
        };

    if identities.len() <= 1 {
        return Err(ApiError::BadRequest(
            "Cannot delete the only authentication method. Add another method first.".into(),
        ));
    }

    // Delete the identity
    match repository::user_auth_identities::delete_identity(
        identity_id,
        &user_uuid_parsed,
        &mut conn,
    ) {
        Ok(count) => {
            if count == 0 {
                Err(ApiError::NotFoundMsg(
                    "Authentication identity not found or doesn't belong to this user".into(),
                ))
            } else {
                Ok(HttpResponse::Ok().json(json!({
                    "status": "success",
                    "message": "Authentication identity deleted successfully"
                })))
            }
        }
        Err(e) => {
            error!(error = ?e, "Error deleting user auth identity");
            Err(ApiError::Internal(
                "Failed to delete authentication identity".into(),
            ))
        }
    }
}

// Upload user profile images (avatar or banner)
pub async fn upload_user_image(
    uuid: web::Path<String>,
    mut payload: Multipart,
    mut tc: TenantConn,
    search_service: web::Data<std::sync::Arc<crate::services::search::SearchService>>,
    type_query: web::Query<UserImageTypeQuery>,
) -> impl Responder {
    let user_uuid = uuid.into_inner();
    let image_type = &type_query.type_; // "avatar" or "banner"

    // Validate that the user exists
    let user_uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid UUID format"),
    };

    // All DB work goes through `tc.run`, which pins the actor + workspace
    // GUCs the audited `users` table requires (the trigger rejects a raw
    // pooled connection with "audit context missing for users.UPDATE").
    let user = match tc.run(|conn| repository::get_user_by_uuid(&user_uuid_parsed, conn)) {
        Ok(user) => user,
        Err(_) => {
            return errors::not_found_msg("User not found");
        }
    };

    // Identity orchestration O5: on hosted, the global AVATAR is owned by the
    // control plane — users change it in their Nosdesk account, which projects
    // it here — so a product-side avatar upload would diverge. Reject it.
    // Banners stay product-owned (the CP models only the avatar).
    if image_type.as_str() == "avatar" && crate::handlers::auth::hosted_local_auth_disabled() {
        return errors::forbidden(
            "Your avatar is managed in your Nosdesk account — update it there.",
        );
    }

    // Determine the upload directory based on image type
    let storage_path = match image_type.as_str() {
        "avatar" => "users/avatars",
        "banner" => "users/banners",
        _ => {
            return errors::bad_request("Invalid image type. Must be 'avatar' or 'banner'");
        }
    };

    // Ensure the directory exists using storage abstraction
    let _full_storage_path = format!("{storage_path}/{user_uuid}");

    debug!(image_type = %image_type, user_uuid = %user_uuid, "Processing image upload");

    // Process the uploaded image (we only handle the first field)
    if let Ok(Some(mut field)) = payload.try_next().await {
        debug!(field_name = ?field.name(), "Received multipart field");

        // Get content type
        let content_type = field
            .content_type()
            .map(|ct| ct.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        debug!(content_type = %content_type, "Content type");

        // Validate content type (only allow images)
        if !content_type.starts_with("image/") {
            return errors::bad_request("Only image files are allowed");
        }

        // Check for HEIC/HEIF - these should be converted on the client side
        if content_type.as_str() == "image/heic" || content_type.as_str() == "image/heif" {
            return errors::bad_request(
                "HEIC/HEIF format should be converted to JPEG on the client side before upload",
            );
        }

        // Extract file extension from content type
        let file_ext = match content_type.as_str() {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => {
                return errors::bad_request(
                    "Unsupported image format. Allowed: JPEG, PNG, GIF, WEBP",
                );
            }
        };

        // No pre-save cleanup needed: the image processor writes to a
        // deterministic per-user key via the storage backend, so a new
        // upload overwrites the previous avatar/banner in place. (The old
        // readdir-based cleanup was filesystem-only and couldn't work on
        // S3/Tigris.)
        let filename = format!("{user_uuid}_{image_type}.{file_ext}");
        let _file_path = format!("{storage_path}/{filename}");

        // Read file data
        let mut file_data = Vec::new();
        while let Some(chunk) = field.next().await {
            let data = match chunk {
                Ok(data) => data,
                Err(e) => {
                    error!(error = ?e, "Error reading chunk");
                    return errors::internal("Error reading uploaded file");
                }
            };
            file_data.extend_from_slice(&data);
        }

        // Process the image based on type
        let (final_url, thumbnail_url) = if image_type == "avatar" {
            // For avatars, process and resize to WebP format with fixed dimensions (200x200 max)
            match crate::utils::image::process_avatar_image(&file_data, &user_uuid, 200).await {
                Ok(Some(avatar_url)) => {
                    debug!(user_uuid = %user_uuid, avatar_url = %avatar_url, "Successfully processed avatar");

                    // Generate thumbnail from the processed avatar
                    let thumb_url = match crate::utils::image::generate_user_avatar_thumbnail(
                        &avatar_url,
                        &user_uuid,
                    )
                    .await
                    {
                        Ok(Some(thumb_url)) => {
                            debug!(user_uuid = %user_uuid, thumb_url = %thumb_url, "Successfully generated thumbnail");
                            Some(thumb_url)
                        }
                        Ok(None) => {
                            warn!(user_uuid = %user_uuid, "Failed to generate thumbnail");
                            None
                        }
                        Err(e) => {
                            error!(user_uuid = %user_uuid, error = %e, "Error generating thumbnail");
                            None
                        }
                    };

                    (avatar_url, thumb_url)
                }
                Ok(None) => {
                    return errors::internal("Failed to process avatar image");
                }
                Err(e) => {
                    error!(user_uuid = %user_uuid, error = %e, "Error processing avatar");
                    return errors::internal(format!("Failed to process avatar image: {}", e));
                }
            }
        } else {
            // For banners, process and resize to WebP format with banner dimensions (1200x400 max)
            match crate::utils::image::process_banner_image(&file_data, &user_uuid, 1200, 400).await
            {
                Ok(Some(banner_url)) => {
                    debug!(user_uuid = %user_uuid, banner_url = %banner_url, "Successfully processed banner");
                    (banner_url, None)
                }
                Ok(None) => {
                    return errors::internal("Failed to process banner image");
                }
                Err(e) => {
                    error!(user_uuid = %user_uuid, error = %e, "Error processing banner");
                    return errors::internal(format!("Failed to process banner image: {}", e));
                }
            }
        };

        // Update the user record with the new image URL

        let user_update = UserUpdate {
            name: None,

            pronouns: None,
            avatar_url: if image_type == "avatar" {
                Some(final_url.clone())
            } else {
                None
            },
            banner_url: if image_type == "banner" {
                Some(final_url.clone())
            } else {
                None
            },
            avatar_thumb: thumbnail_url,
            microsoft_uuid: None, // Don't update Microsoft UUID in regular user updates
            updated_at: Some(chrono::Utc::now().naive_utc()),
        };

        match tc.run(|conn| {
            repository::update_user(
                &user.uuid,
                user_update,
                conn,
                Some(search_service.get_ref()),
            )
        }) {
            Ok(updated_user) => {
                return HttpResponse::Ok().json(json!({
                    "status": "success",
                    "message": format!("User {} updated successfully", image_type),
                    "url": final_url,
                    "user": UserResponse::from(updated_user)
                }));
            }
            Err(e) => {
                error!(error = ?e, "Error updating user");
                return errors::internal("Error updating user record");
            }
        }
    }

    // If we get here, no file was provided
    errors::bad_request("No image file provided")
}

#[derive(serde::Deserialize)]
pub struct UserImageTypeQuery {
    pub type_: String, // "avatar" or "banner"
}

/// Clean up all stale user images (admin endpoint)
pub async fn cleanup_stale_images(
    req: HttpRequest,
    db_pool: web::Data<crate::db::Pool>,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&db_pool)?;

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return Err(ApiError::Unauthorized("Authentication required".into())),
    };

    if !is_platform_admin(&claims) {
        return Err(ApiError::Forbidden(
            "Only administrators can cleanup stale images".into(),
        ));
    }

    // Get all users to know which files should exist
    let users = match repository::get_users(&mut conn) {
        Ok(users) => users,
        Err(e) => {
            error!(error = ?e, "Error fetching users");
            return Err(ApiError::Internal("Failed to fetch users".into()));
        }
    };

    let mut cleanup_stats = CleanupStats {
        avatars_removed: 0,
        banners_removed: 0,
        thumbnails_removed: 0,
        total_files_checked: 0,
        errors: Vec::new(),
    };

    // Clean up avatar directory
    if let Err(e) = cleanup_directory_stale_files(
        "uploads/users/avatars",
        &users,
        &["avatar", "48x48", "120x120", "default"],
        &mut cleanup_stats,
    )
    .await
    {
        cleanup_stats
            .errors
            .push(format!("Avatar cleanup error: {e}"));
    }

    // Clean up banner directory
    if let Err(e) = cleanup_directory_stale_files(
        "uploads/users/banners",
        &users,
        &["banner"],
        &mut cleanup_stats,
    )
    .await
    {
        cleanup_stats
            .errors
            .push(format!("Banner cleanup error: {e}"));
    }

    // Clean up thumbnail directory
    if let Err(e) = cleanup_directory_stale_files(
        "uploads/users/thumbs",
        &users,
        &["thumb"],
        &mut cleanup_stats,
    )
    .await
    {
        cleanup_stats
            .errors
            .push(format!("Thumbnail cleanup error: {e}"));
    }

    Ok(HttpResponse::Ok().json(json!({
        "status": "success",
        "message": "Stale image cleanup completed",
        "stats": {
            "avatars_removed": cleanup_stats.avatars_removed,
            "banners_removed": cleanup_stats.banners_removed,
            "thumbnails_removed": cleanup_stats.thumbnails_removed,
            "total_files_checked": cleanup_stats.total_files_checked,
            "errors": cleanup_stats.errors
        }
    })))
}

/// POST /users/regenerate-thumbnails — admin maintenance action that
/// rebuilds avatar thumbnails missing on disk or unset in the DB. Shares
/// the backfill routine with the restore paths and the daily scheduled
/// sweep, so behaviour stays consistent across all three triggers.
/// Idempotent: regenerates only what's missing.
pub async fn regenerate_avatar_thumbnails(
    req: HttpRequest,
    db_pool: web::Data<crate::db::Pool>,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&db_pool)?;

    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return Err(ApiError::Unauthorized("Authentication required".into())),
    };

    if !is_platform_admin(&claims) {
        return Err(ApiError::Forbidden(
            "Only administrators can regenerate thumbnails".into(),
        ));
    }

    let stats = crate::services::avatar_thumbnails::backfill_thumbnails(
        &mut conn,
        crate::services::avatar_thumbnails::BackfillMode::MissingOnly,
        "handler:thumbnail_regen",
    )
    .await;

    Ok(HttpResponse::Ok().json(json!({
        "success": true,
        "stats": {
            "checked": stats.checked,
            "regenerated": stats.regenerated,
            "failed": stats.failed,
        }
    })))
}

#[derive(Debug)]
struct CleanupStats {
    avatars_removed: usize,
    banners_removed: usize,
    thumbnails_removed: usize,
    total_files_checked: usize,
    errors: Vec<String>,
}

/// Clean up stale files in a specific directory
async fn cleanup_directory_stale_files(
    dir_path: &str,
    users: &[crate::models::User],
    valid_suffixes: &[&str],
    stats: &mut CleanupStats,
) -> Result<(), String> {
    use std::collections::HashSet;
    use tokio::fs;

    // Create a set of valid file prefixes (user UUIDs)
    let valid_uuids: HashSet<String> = users
        .iter()
        .map(|u| utils::uuid_to_string(&u.uuid))
        .collect();

    // Read the directory
    let mut dir = match fs::read_dir(dir_path).await {
        Ok(dir) => dir,
        Err(_) => return Ok(()), // Directory doesn't exist, nothing to clean
    };

    while let Ok(Some(entry)) = dir.next_entry().await {
        if let Some(filename) = entry.file_name().to_str() {
            stats.total_files_checked += 1;

            // Check if this file should be kept
            let should_keep = should_keep_file(filename, &valid_uuids, valid_suffixes);

            if !should_keep {
                let file_path = entry.path();
                debug!(file_path = ?file_path, "Removing stale image file");

                match fs::remove_file(&file_path).await {
                    Ok(_) => {
                        if dir_path.contains("avatars") {
                            stats.avatars_removed += 1;
                        } else if dir_path.contains("banners") {
                            stats.banners_removed += 1;
                        } else if dir_path.contains("thumbs") {
                            stats.thumbnails_removed += 1;
                        }
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to remove {file_path:?}: {e}");
                        warn!(file_path = ?file_path, error = %e, "Failed to remove file");
                        stats.errors.push(error_msg);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Determine if a file should be kept based on naming patterns
fn should_keep_file(
    filename: &str,
    valid_uuids: &HashSet<String>,
    valid_suffixes: &[&str],
) -> bool {
    // Skip hidden files like .DS_Store
    if filename.starts_with('.') {
        return true;
    }

    // Skip non-image files
    if !filename.contains('.') {
        return true;
    }

    // Extract the base name without extension
    let base_name = filename.split('.').next().unwrap_or("");

    // Check for new format: {uuid}_{suffix} (like uuid_avatar.webp or uuid_thumb.webp)
    if let Some(underscore_pos) = base_name.find('_') {
        let uuid_part = &base_name[..underscore_pos];
        let suffix_part = &base_name[underscore_pos + 1..];

        // Check if this matches our expected NEW pattern
        if valid_uuids.contains(uuid_part) && valid_suffixes.contains(&suffix_part) {
            debug!(filename = %filename, "Keeping new format file");
            return true; // Keep this file
        }

        // Check for old format patterns that should be removed
        // Old patterns: {uuid}_120x120.jpg, {uuid}_48x48.jpg, {uuid}_{random-uuid}_banner.jpg
        if valid_uuids.contains(uuid_part) {
            // This is for a valid user but in old format - remove it
            if suffix_part.contains("x") || suffix_part.len() > 20 {
                // Likely old format
                debug!(filename = %filename, "Removing old format file");
                return false;
            }
        }
    }

    // Check for files that don't start with a valid UUID - these are definitely stale
    let parts: Vec<&str> = base_name.split('_').collect();
    if !parts.is_empty() && !valid_uuids.contains(parts[0]) {
        debug!(filename = %filename, "Removing file with invalid UUID");
        return false;
    }

    // If the pattern cannot be determined clearly, keep the file to be safe
    debug!(filename = %filename, "Keeping unknown pattern file");
    true
}

pub async fn update_user_by_uuid(
    mut tc: TenantConn,
    search_service: web::Data<Arc<SearchService>>,
    req: HttpRequest,
    path: web::Path<String>,
    user_data: web::Json<UserUpdateWithPassword>,
) -> impl Responder {
    let user_uuid = path.into_inner();

    let claims = match crate::utils::rbac::require_auth(&req) {
        Ok(claims) => claims,
        Err(_) => return errors::unauthorized("Authentication required"),
    };

    // First get the user by UUID to get the ID
    let user_uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid UUID format"),
    };

    // Authorization: Users can only update their own profile, admins can update anyone
    if claims.sub != user_uuid && !is_platform_admin(&claims) {
        return errors::forbidden("You can only update your own profile");
    }

    // Identity orchestration O3: on hosted, the global display NAME is owned by
    // the control plane (projected here, read-only in the product) — reject a
    // name edit so the projected cache can't re-diverge; the user changes it in
    // their Nosdesk account. Pronouns/avatar and preferences stay editable
    // (avatar moves to the CP later, in O5). Self-hosted is unaffected.
    if user_data.name.is_some() && crate::handlers::auth::hosted_local_auth_disabled() {
        return errors::forbidden(
            "Your display name is managed in your Nosdesk account — update it there.",
        );
    }

    // Every DB operation runs through `tc.run`, which wraps the work in
    // a transaction with the actor + workspace GUCs set. The audited
    // `users` table needs `app.workspace_id` pinned for the NDX01
    // trigger to accept writes; the extractor provides it from the
    // request's WorkspaceContext, so there's no raw connection to
    // forget to wrap.
    let user = match tc.run(|conn| repository::get_user_by_uuid(&user_uuid_parsed, conn)) {
        Ok(user) => user,
        Err(_) => return errors::not_found_msg("User not found"),
    };

    // Password change: rotate the local auth identity. The hash is
    // computed outside the transaction (bcrypt is deliberately slow);
    // the identity swap (delete-then-create, or create-if-absent) runs
    // as one `tc.run` transaction so a failure can't leave the user
    // with no local identity and locked out.
    if let Some(password) = &user_data.password {
        use crate::models::NewUserAuthIdentity;
        use bcrypt::hash;

        // Setting a local password is refused in hosted mode (identity is
        // SSO-only). Guard before the transaction so the repo gate below is
        // never the one to trip.
        if crate::handlers::auth::hosted_local_auth_disabled() {
            return errors::forbidden("Local password authentication is disabled");
        }

        let password_hash = match hash(password, DEFAULT_COST) {
            Ok(hash) => hash,
            Err(_) => return errors::internal("Error hashing password"),
        };

        let user_uuid_for_identity = user.uuid;
        let identity_result = tc.run(|conn| {
            let auth_identities = repository::user_auth_identities::get_user_identities(
                &user_uuid_for_identity,
                conn,
            )?;
            let local_identity = auth_identities
                .iter()
                .find(|identity| identity.provider_type == "local");

            let new_auth_identity = match local_identity {
                Some(identity) => {
                    // Replace the existing local identity in place.
                    repository::user_auth_identities::delete_identity(
                        identity.id,
                        &user_uuid_for_identity,
                        conn,
                    )?;
                    NewUserAuthIdentity {
                        user_uuid: user_uuid_for_identity,
                        provider_type: identity.provider_type.clone(),
                        external_id: identity.external_id.clone(),
                        email: identity.email.clone(),
                        metadata: identity.metadata.clone(),
                        password_hash: Some(password_hash),
                        workspace_id: None,
                    }
                }
                None => NewUserAuthIdentity {
                    user_uuid: user_uuid_for_identity,
                    provider_type: "local".to_string(),
                    external_id: Uuid::now_v7().to_string(),
                    email: None, // Email in user_emails table
                    metadata: None,
                    password_hash: Some(password_hash),
                    workspace_id: None,
                },
            };
            repository::user_auth_identities::create_local_identity(new_auth_identity, conn)
                .map_err(|e| e.into_diesel())?;
            Ok(())
        });

        if let Err(e) = identity_result {
            error!(error = ?e, "Error rotating local auth identity");
            return errors::internal("Error updating password");
        }
    }

    // Validate theme if provided (allow any non-empty string as theme ID)
    if let Some(ref theme) = user_data.theme {
        if theme.is_empty() || theme.len() > 50 {
            return errors::bad_request("Theme must be a non-empty string up to 50 characters");
        }
    }

    // Validate dashboard_layout shape so malformed client payloads
    // can't land garbage JSON in the column.
    if let Some(ref layout) = user_data.dashboard_layout {
        if let Err(msg) = validate_dashboard_layout(layout) {
            return errors::bad_request(msg);
        }
    }

    // Validate signature template tokens up front so a typo like
    // `{{tech_naem}}` fails at save time rather than landing in an
    // outbound customer reply verbatim. Empty / pure-whitespace
    // signatures skip validation since they'll be normalized to
    // None below.
    if let Some(ref sig) = user_data.signature {
        if !sig.trim().is_empty() {
            let unknown = crate::utils::template_variables::unknown_variables(
                sig,
                crate::utils::template_variables::SIGNATURE_VARIABLES,
            );
            if !unknown.is_empty() {
                return errors::bad_request(format!(
                    "Unknown signature variables: {}. Supported: {}.",
                    unknown.join(", "),
                    crate::utils::template_variables::SIGNATURE_VARIABLES.join(", ")
                ));
            }
        }
    }

    // Role change (W2 two-axis model). A platform admin editing another
    // user may change their role; the request role string is re-derived
    // into (platform_role, workspace_role) and both are rewritten, scoped
    // to the request's workspace, mirroring the bulk set-role path.
    //
    // A role field on a self-update is ignored rather than applied: you
    // can't change your own role here (that would be a self-lockout
    // footgun), and ignoring it keeps a normal profile edit working even
    // when the client includes the field. Non-admins only ever reach this
    // handler for their own UUID (the auth check above), so the guard
    // below covers both cases.
    if let Some(role_str) = user_data
        .role
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if claims.sub != user_uuid && is_platform_admin(&claims) {
            let (platform_role_enum, workspace_role_enum) = match utils::parse_roles(role_str) {
                Ok(roles) => roles,
                Err(_) => return errors::bad_request("Invalid role value"),
            };
            let platform_role = platform_role_enum.as_str();
            let workspace_role = workspace_role_enum.as_str();

            // Last-admin guard: refuse to move the workspace's only admin
            // off the admin role, so a workspace can't be left with nobody
            // able to manage it.
            if workspace_role != "admin" {
                #[derive(diesel::QueryableByName)]
                struct AdminGuard {
                    #[diesel(sql_type = diesel::sql_types::BigInt)]
                    admin_count: i64,
                    #[diesel(sql_type = diesel::sql_types::Bool)]
                    target_is_admin: bool,
                }
                let guard = tc.run(|conn| {
                    diesel::sql_query(
                        // Removed admins must not prop up the count, or the
                        // last real admin could be demoted behind their ghost.
                        "SELECT \
                           (SELECT COUNT(*) FROM workspace_members \
                              WHERE workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int \
                                AND role = 'admin' AND removed_at IS NULL) AS admin_count, \
                           EXISTS(SELECT 1 FROM workspace_members \
                              WHERE workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int \
                                AND user_uuid = $1 AND role = 'admin' AND removed_at IS NULL) AS target_is_admin",
                    )
                    .bind::<diesel::sql_types::Uuid, _>(user_uuid_parsed)
                    .get_result::<AdminGuard>(conn)
                });
                match guard {
                    Ok(g) if g.target_is_admin && g.admin_count <= 1 => {
                        return errors::bad_request(
                            "Cannot remove the last administrator from this workspace",
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!(error = ?e, "Failed to evaluate last-admin guard");
                        return errors::internal("Error updating user role");
                    }
                }
            }

            // Rewrite platform_role on the audited users row and the
            // workspace_members role for the request's workspace, both
            // inside the actor + workspace-scoped transaction.
            let ws_id = tc.workspace_id().unwrap_or_default();

            // Product authority: set_user_roles refuses a role change touching a
            // control-plane-owned staff seat in hosted (current OR new is staff).
            let role_result = tc.run(|conn| {
                repository::users::set_user_roles(
                    conn,
                    ws_id,
                    user_uuid_parsed,
                    platform_role,
                    workspace_role,
                    repository::workspaces::SeatWriteAuthority::Product,
                )
            });
            match role_result {
                Ok(repository::users::SetUserRolesOutcome::Applied) => {}
                Ok(repository::users::SetUserRolesOutcome::ExternallyManaged) => {
                    return errors::externally_managed();
                }
                Err(e) => {
                    error!(error = ?e, user_uuid = %user_uuid_parsed, "Failed to update user role");
                    return errors::internal("Error updating user role");
                }
            }
        }
    }

    // Update user (core identity fields only; preferences land
    // separately in user_preferences below). Role was handled above.
    let user_update = UserUpdate {
        name: user_data.name.clone(),
        pronouns: user_data.pronouns.clone(),
        avatar_url: user_data.avatar_url.clone(),
        banner_url: user_data.banner_url.clone(),
        avatar_thumb: user_data.avatar_thumb.clone(),
        microsoft_uuid: None,
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    // Compose the preferences patch. Empty-string signature /
    // locale / timezone are treated as "clear back to None"
    // (matches the textarea-emptied semantic for signature, and
    // gives the settings UI a way to revert to site defaults
    // without a separate API call).
    let empty_to_none = |s: &String| -> Option<String> {
        if s.trim().is_empty() {
            None
        } else {
            Some(s.clone())
        }
    };
    let prefs_update = crate::models::UpdateUserPreferences {
        theme: user_data.theme.clone().map(Some),
        signature: user_data.signature.as_ref().map(empty_to_none),
        dashboard_layout: user_data.dashboard_layout.clone().map(Some),
        locale: user_data.locale.as_ref().map(empty_to_none),
        timezone: user_data.timezone.as_ref().map(empty_to_none),
    };
    let prefs_changed = user_data.theme.is_some()
        || user_data.signature.is_some()
        || user_data.dashboard_layout.is_some()
        || user_data.locale.is_some()
        || user_data.timezone.is_some();

    // Apply preference changes (if any) BEFORE the core-user update so
    // the `user.updated` sync action that `update_user` emits reads the
    // fresh dashboard_layout from user_preferences. Best-effort: a
    // transient prefs-table write failure logs but doesn't fail the
    // request (the preference fields are independent of the core row).
    if prefs_changed {
        if let Err(e) =
            tc.run(|conn| repository::user_preferences::update(conn, user.uuid, prefs_update))
        {
            tracing::error!(
                error = ?e,
                user_uuid = %user.uuid,
                "Failed to update user_preferences before user update",
            );
        }
    }

    let update_result = tc.run(|conn| {
        repository::update_user(
            &user.uuid,
            user_update,
            conn,
            Some(search_service.get_ref()),
        )
    });
    match update_result {
        Ok(updated_user) => {
            // Profile + preference changes (name, role, pronouns,
            // avatar, dashboard_layout) reach clients through the sync
            // pool: update_user emits a `user.updated` sync action
            // carrying the full row (incl. dashboard_layout), so a
            // user's own sessions mirror the change. No discrete SSE.

            // Re-index the updated user in search
            let primary_email = tc
                .run(|conn| {
                    Ok(repository::user_helpers::get_primary_email(
                        &updated_user.uuid,
                        conn,
                    ))
                })
                .unwrap_or(None::<String>);
            indexing_tasks::spawn_index_user(
                search_service.get_ref().clone(),
                updated_user.clone(),
                primary_email,
            );

            // Use helper function to fetch primary email from user_emails table
            let user_response = tc.run(|conn| {
                Ok(repository::user_helpers::get_user_with_primary_email(
                    updated_user.clone(),
                    conn,
                ))
            });
            match user_response {
                Ok(resp) => HttpResponse::Ok().json(resp),
                Err(e) => {
                    error!(error = ?e, user_uuid = %updated_user.uuid, "Error loading updated user response");
                    errors::internal("Error updating user")
                }
            }
        }
        Err(e) => {
            error!(error = ?e, user_uuid = %user.uuid, "Error updating user");
            errors::internal("Error updating user")
        }
    }
}

/// Get user email addresses
pub async fn get_user_emails(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<String>, // User UUID
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&db_pool)?;

    let user_uuid = path.into_inner();

    let claims = crate::utils::rbac::require_auth(&req)?;

    // Check authorization (user can access their own emails, admins can access any)
    if claims.sub != user_uuid && !is_platform_admin(&claims) {
        return Err(ApiError::Forbidden(
            "Not authorized to access this resource".into(),
        ));
    }

    // Get user emails
    let uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID format".into())),
    };

    // Get user first to ensure they exist
    let _user = match repository::get_user_by_uuid(&uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return Err(ApiError::NotFoundMsg("User not found".into())),
    };

    // Get emails from user_emails table (single source of truth)
    let emails = user_emails_repo::get_user_emails_by_uuid(&mut conn, &uuid_parsed)
        .unwrap_or_else(|_| Vec::new());

    Ok(HttpResponse::Ok().json(json!({
        "status": "success",
        "emails": emails
    })))
}

/// Add a new email address for a user
pub async fn add_user_email(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    ws: WorkspaceContext,
    path: web::Path<String>,
    email_data: web::Json<serde_json::Value>,
) -> Result<HttpResponse, ApiError> {
    let user_uuid = path.into_inner();
    let mut conn = helpers::db_conn(&db_pool)?;

    let claims = crate::utils::rbac::require_auth(&req)?;

    // Authorization: Users can only add emails to their own account, admins can add to anyone
    if claims.sub != user_uuid && !is_platform_admin(&claims) {
        return Err(ApiError::Forbidden("Not authorized".into()));
    }

    // Extract email from request
    let email = match email_data.get("email").and_then(|e| e.as_str()) {
        Some(e) => e.trim().to_lowercase(),
        None => return Err(ApiError::BadRequest("Email is required".into())),
    };

    // Validate email format
    if !email.contains('@') || !email.contains('.') {
        return Err(ApiError::BadRequest("Invalid email format".into()));
    }

    // Get user ID
    let uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID format".into())),
    };

    let user = match repository::get_user_by_uuid(&uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return Err(ApiError::NotFoundMsg("User not found".into())),
    };

    // Check if email already exists
    if user_emails_repo::find_user_by_any_email(&mut conn, &email).is_ok() {
        return Err(ApiError::BadRequest("Email address already in use".into()));
    }

    // Create new email
    let new_email = crate::models::NewUserEmail {
        user_uuid: user.uuid,
        email: email.clone(),
        email_type: "personal".to_string(),
        is_primary: false,
        is_verified: false,
        source: Some("manual".to_string()),
    };

    match user_emails_repo::add_email(&mut conn, &new_email) {
        Ok(created_email) => {
            // Send the confirmation immediately: an address that arrives
            // unverified with no way to prove itself is the state this whole
            // flow exists to end. Best-effort, so a mail failure does not undo
            // the add; the profile offers a resend.
            crate::services::email_verification::send_verification(
                &mut conn,
                &user,
                &created_email,
                ws.workspace_id,
                crate::utils::client_ip::from_http_request(&req)
                    .map(|ip| ip.to_string())
                    .as_deref(),
                req.headers()
                    .get(actix_web::http::header::USER_AGENT)
                    .and_then(|v| v.to_str().ok()),
            );
            Ok(HttpResponse::Created().json(json!({
                "status": "success",
                "message": "Email added successfully",
                "email": created_email
            })))
        }
        Err(e) => {
            error!(error = ?e, "Error adding email");
            Err(ApiError::Internal("Failed to add email".into()))
        }
    }
}

/// Update an email address (set as primary)
pub async fn update_user_email(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<(String, i32)>,
    update_data: web::Json<serde_json::Value>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, email_id) = path.into_inner();
    let mut conn = helpers::db_conn(&db_pool)?;

    let claims = crate::utils::rbac::require_auth(&req)?;

    // Authorization
    if claims.sub != user_uuid && !is_platform_admin(&claims) {
        return Err(ApiError::Forbidden("Not authorized".into()));
    }

    // Get user
    let uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID format".into())),
    };

    let user = match repository::get_user_by_uuid(&uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return Err(ApiError::NotFoundMsg("User not found".into())),
    };

    // If setting as primary, unset other primary emails first
    if update_data
        .get("is_primary")
        .and_then(|p| p.as_bool())
        .unwrap_or(false)
    {
        let _ = user_emails_repo::clear_primary(&mut conn, &user.uuid);
    }

    // `is_verified` is deliberately NOT read from the body. This endpoint is
    // authorised by "you are this user", so accepting it let anyone mark their
    // own address verified, which is the whole of what verification asserts.
    //
    // The flag is not cosmetic: `user_helpers::find_verified_user_by_email` is
    // the inbound-mail impersonation guard and trusts it, so a self-verified
    // address makes the channel pipeline attribute mail from that address to
    // the account that claimed it.
    //
    // Ownership is proved by a challenge to the address, never asserted by its
    // claimant. The two writers that remain both rest on such a proof: account
    // creation, from the identity provider's own `email_verified` claim, and
    // `user_emails::mark_primary_verified` on invitation accept, where
    // receiving the invite is the proof.
    let email_update = crate::models::UserEmailUpdate {
        is_primary: update_data.get("is_primary").and_then(|p| p.as_bool()),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    match user_emails_repo::update_email(&mut conn, email_id, &email_update) {
        Ok(updated_email) => Ok(HttpResponse::Ok().json(json!({
            "status": "success",
            "message": "Email updated successfully",
            "email": updated_email
        }))),
        Err(e) => {
            error!(error = ?e, "Error updating email");
            Err(ApiError::Internal("Failed to update email".into()))
        }
    }
}

/// Re-send the confirmation link for an unverified address.
///
/// Throttled per user per hour, the same way password reset and the portal
/// magic link are. This one needs it more than either: an unconfirmed address
/// does not belong to the person asking, so an unthrottled resend is a way to
/// make us repeatedly email a stranger. The `/api` scope limiter bounds
/// request rate generally, but it is not a per-address cap on outbound mail.
///
/// Over the limit returns success. Whether an address is at its resend ceiling
/// is not something a caller should be able to probe for, and the honest
/// alternative leaks it.
pub async fn resend_user_email_verification(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    ws: WorkspaceContext,
    path: web::Path<(String, i32)>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, email_id) = path.into_inner();
    let mut conn = helpers::db_conn(&db_pool)?;

    let claims = crate::utils::rbac::require_auth(&req)?;
    if claims.sub != user_uuid && !is_platform_admin(&claims) {
        return Err(ApiError::Forbidden("Not authorized".into()));
    }

    let uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID format".into())),
    };
    let user = match repository::get_user_by_uuid(&uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return Err(ApiError::NotFoundMsg("User not found".into())),
    };

    let email = match user_emails_repo::get_user_emails_by_uuid(&mut conn, &uuid_parsed) {
        Ok(rows) => match rows.into_iter().find(|e| e.id == email_id) {
            Some(e) => e,
            None => return Err(ApiError::NotFoundMsg("Email not found".into())),
        },
        Err(e) => {
            error!(error = ?e, "Error loading emails for resend");
            return Err(ApiError::Internal("Failed to load email".into()));
        }
    };

    if email.is_verified {
        return Err(ApiError::BadRequest(
            "That address is already confirmed".into(),
        ));
    }

    // Same window and ceiling as password reset, for the same reason.
    let since = chrono::Utc::now() - chrono::Duration::hours(1);
    let recent = match crate::repository::reset_tokens::count_recent_tokens(
        &mut conn,
        uuid_parsed,
        crate::utils::reset_tokens::TokenType::EmailVerification.as_str(),
        since,
    ) {
        Ok(n) => n,
        Err(e) => {
            error!(error = ?e, "Error counting recent verification tokens");
            return Err(ApiError::Internal("Failed to send confirmation".into()));
        }
    };
    if recent >= MAX_VERIFICATION_RESENDS_PER_HOUR {
        warn!(
            user_uuid = %uuid_parsed,
            "Rate limit exceeded for email verification resend"
        );
        return Ok(HttpResponse::Ok().json(json!({
            "status": "success",
            "message": "Confirmation email sent"
        })));
    }

    // Every outstanding link for this user is superseded. Without this, an
    // address removed and re-added, or simply resent a few times, leaves older
    // tokens live, and each one is a standing claim on whatever row it names.
    if let Err(e) = crate::repository::reset_tokens::invalidate_tokens_by_type(
        &mut conn,
        uuid_parsed,
        crate::utils::reset_tokens::TokenType::EmailVerification.as_str(),
    ) {
        error!(error = ?e, "Error invalidating prior verification tokens");
        return Err(ApiError::Internal("Failed to send confirmation".into()));
    }

    crate::services::email_verification::send_verification(
        &mut conn,
        &user,
        &email,
        ws.workspace_id,
        crate::utils::client_ip::from_http_request(&req)
            .map(|ip| ip.to_string())
            .as_deref(),
        req.headers()
            .get(actix_web::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok()),
    );

    Ok(HttpResponse::Ok().json(json!({
        "status": "success",
        "message": "Confirmation email sent"
    })))
}

/// Delete an email address
pub async fn delete_user_email(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<(String, i32)>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, email_id) = path.into_inner();
    let mut conn = helpers::db_conn(&db_pool)?;

    let claims = crate::utils::rbac::require_auth(&req)?;

    // Authorization
    if claims.sub != user_uuid && !is_platform_admin(&claims) {
        return Err(ApiError::Forbidden("Not authorized".into()));
    }

    // Get user and verify email belongs to them
    let uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID format".into())),
    };

    let user = match repository::get_user_by_uuid(&uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return Err(ApiError::NotFoundMsg("User not found".into())),
    };

    // Check if email is primary
    let email: crate::models::UserEmail =
        match user_emails_repo::get_email_by_id(&mut conn, email_id) {
            Ok(email) => email,
            Err(_) => return Err(ApiError::NotFoundMsg("Email not found".into())),
        };

    if email.user_uuid != user.uuid {
        return Err(ApiError::Forbidden(
            "Email does not belong to this user".into(),
        ));
    }

    if email.is_primary {
        return Err(ApiError::BadRequest(
            "Cannot delete primary email address".into(),
        ));
    }

    // Delete the email
    match user_emails_repo::delete_email(&mut conn, email_id) {
        Ok(_) => Ok(HttpResponse::Ok().json(json!({
            "status": "success",
            "message": "Email deleted successfully"
        }))),
        Err(e) => {
            error!(error = ?e, "Error deleting email");
            Err(ApiError::Internal("Failed to delete email".into()))
        }
    }
}

/// Resend invitation email to a user who hasn't set up their account yet
pub async fn resend_invitation(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<String>, // User UUID
) -> Result<HttpResponse, ApiError> {
    let user_uuid = path.into_inner();
    let mut conn = helpers::db_conn(&db_pool)?;
    // Pin the request's workspace so the branding read and invitation-email
    // enqueue (both RLS-isolated) are scoped; the pool clears app.workspace_id
    // on checkout, so an unpinned enqueue fails the NOT NULL workspace default.
    helpers::pin_request_workspace(&req, &mut conn);

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return Err(ApiError::Unauthorized("Authentication required".into())),
    };

    // Only admins can resend invitations
    // Check email configuration. EmailService::from_env builds the SMTP
    // transport and reports is_configured for it.
    let smtp_configured = crate::utils::email::EmailService::from_env()
        .map(|s| s.is_configured())
        .unwrap_or(false);

    if !smtp_configured {
        return Err(ApiError::BadRequest(
            "Email is not configured. Cannot send invitation.".into(),
        ));
    }

    // Get the user
    let uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID format".into())),
    };

    // Was platform-admin-only and looked up any global uuid, then invalidated
    // its invitation tokens with no membership check. Now a workspace admin may
    // resend only for a member of their OWN workspace (the isolation boundary);
    // hosted invites are control-plane seats, so this is self-hosted only.
    // (Token invalidation remains account-wide; per-workspace scoping is deferred.)
    helpers::authorize_target_user_action(&req, &db_pool, &claims, uuid_parsed, false)?;

    let user = match repository::get_user_by_uuid(&uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return Err(ApiError::NotFoundMsg("User not found".into())),
    };

    // Check if user already has a password set (completed setup)
    let auth_identities =
        repository::user_auth_identities::get_user_identities(&user.uuid, &mut conn)
            .unwrap_or_default();

    let has_password = auth_identities
        .iter()
        .any(|identity| identity.provider_type == "local" && identity.password_hash.is_some());

    if has_password {
        return Err(ApiError::BadRequest(
            "User has already completed account setup. Cannot resend invitation.".into(),
        ));
    }

    // Get user's primary email - try to find one marked as primary first
    let emails = match user_emails_repo::get_user_emails_by_uuid(&mut conn, &user.uuid) {
        Ok(emails) if !emails.is_empty() => emails,
        _ => return Err(ApiError::BadRequest("User has no email address".into())),
    };

    // Find primary email or use first one
    let user_email = emails
        .iter()
        .find(|e| e.is_primary)
        .map(|e| e.email.clone())
        .unwrap_or_else(|| emails[0].email.clone());

    // Get the admin user's name for the invitation email
    let admin_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::Internal("Invalid admin UUID".into())),
    };

    let admin_name = match repository::get_user_by_uuid(&admin_uuid, &mut conn) {
        Ok(admin) => admin.name,
        Err(_) => "An administrator".to_string(),
    };

    // Invalidate any existing invitation tokens for this user
    if let Err(e) =
        repository::reset_tokens::invalidate_tokens_by_type(&mut conn, user.uuid, "invitation")
    {
        warn!(user_uuid = %user.uuid, error = ?e, "Failed to invalidate old invitation tokens");
        // Continue anyway - old tokens will expire naturally
    }

    // Send invitation using shared helper
    match send_user_invitation(
        &mut conn,
        &req,
        user.uuid,
        &user_email,
        &user.name,
        &admin_name,
    )
    .await
    {
        SendInvitationResult::Success => {
            info!(email = %user_email, user_name = %user.name, "Invitation email resent");
            Ok(HttpResponse::Ok().json(json!({
                "status": "success",
                "message": "Invitation email sent successfully",
                "email": user_email
            })))
        }
        SendInvitationResult::TokenStorageError(e) => {
            error!(error = %e, "Error storing invitation token");
            Err(ApiError::Internal("Error creating invitation".into()))
        }
        SendInvitationResult::EmailServiceError(e) => {
            error!(error = %e, "Error initializing email service");
            Err(ApiError::Internal("Error sending invitation email".into()))
        }
        SendInvitationResult::EmailSendError(e) => {
            error!(error = %e, "Error sending invitation email");
            Err(ApiError::Internal("Failed to send invitation email".into()))
        }
    }
}

/// Get user with all email addresses
pub async fn get_user_with_emails(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<String>, // User UUID
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&db_pool)?;

    // Pin the request's workspace so the returned user's workspace_role
    // resolves under RLS (workspace_members is workspace-isolated).
    helpers::pin_request_workspace(&req, &mut conn);

    let user_uuid = path.into_inner();

    let claims = crate::utils::rbac::require_auth(&req)?;

    // Check authorization
    if claims.sub != user_uuid && !is_platform_admin(&claims) {
        return Err(ApiError::Forbidden(
            "Not authorized to access this resource".into(),
        ));
    }

    // Get user
    let uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID format".into())),
    };

    let user = match repository::get_user_by_uuid(&uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return Err(ApiError::NotFoundMsg("User not found".into())),
    };

    // Get user emails
    let emails = match user_emails_repo::get_user_emails_by_uuid(&mut conn, &user.uuid) {
        Ok(emails) => emails,
        Err(e) => {
            error!(user_uuid = %user.uuid, error = ?e, "Error fetching emails for user");
            Vec::new() // Return empty vec if error fetching emails
        }
    };

    // Get user response with primary email populated
    let user_response = repository::user_helpers::get_user_with_primary_email(user, &mut conn);

    let user_with_emails = crate::models::UserWithEmails {
        user: user_response,
        emails,
    };

    Ok(HttpResponse::Ok().json(user_with_emails))
}

#[derive(Deserialize)]
pub struct ProfileQuery {
    /// Comma-separated sub-resource keys to include (`devices,
    /// groups, emails, counts`). Omit to get every group; pass an
    /// empty value to get just `user`.
    pub include: Option<String>,
}

/// Bundled profile read for the user profile page. One request,
/// one cache entry, only the requested sub-resources serialised.
/// Self or admin only.
pub async fn get_user_profile_bundle(
    pool: web::Data<crate::db::Pool>,
    path: web::Path<String>,
    query: web::Query<ProfileQuery>,
    auth: crate::extractors::AuthContext,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let uuid_str = path.into_inner();
    let user_uuid_parsed = match utils::parse_uuid(&uuid_str) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID format".into())),
    };

    if user_uuid_parsed != auth.user_uuid && !auth.is_workspace_admin() {
        return Err(ApiError::Forbidden(
            "Not authorized to view this profile".into(),
        ));
    }

    let mut groups = parse_profile_include(query.include.as_deref())?;

    // Privacy (identity orchestration O6): the full verified email set is
    // self / platform-admin only. A WORKSPACE admin viewing another member's
    // profile must not receive it — with the control plane projecting a user's
    // whole verified set into `user_emails`, that would leak every address a
    // member owns; admins are only ever meant to see the address a seat was
    // invited to (surfaced elsewhere, not from this set). Strip the group
    // rather than 403 the whole bundle so the other groups still load.
    if user_uuid_parsed != auth.user_uuid && !auth.is_platform_admin() {
        groups.remove(&crate::repository::user_profile::ProfileGroup::Emails);
    }

    let mut conn = helpers::db_conn(&pool)?;
    // Pin the request's workspace so the bundle's workspace_role (and other
    // RLS-scoped reads) resolve to the caller's workspace.
    helpers::pin_request_workspace(&req, &mut conn);

    match crate::repository::user_profile::compute(&mut conn, &user_uuid_parsed, &groups) {
        Ok(Some(bundle)) => Ok(HttpResponse::Ok().json(bundle)),
        Ok(None) => Err(ApiError::NotFoundMsg("User not found".into())),
        Err(e) => {
            error!(user_uuid = %user_uuid_parsed, error = ?e, "Failed to compute profile bundle");
            Err(ApiError::Internal("Failed to load profile".into()))
        }
    }
}

/// Returns 400 with the offending key when an unknown group is
/// requested, so a registry typo on the frontend fails loud
/// instead of silently dropping data.
fn parse_profile_include(
    raw: Option<&str>,
) -> Result<HashSet<crate::repository::user_profile::ProfileGroup>, ApiError> {
    use crate::repository::user_profile::ProfileGroup;
    let Some(raw) = raw else {
        return Ok(ProfileGroup::all());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(HashSet::new());
    }
    let mut out = HashSet::new();
    for token in trimmed.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match ProfileGroup::parse(token) {
            Some(g) => {
                out.insert(g);
            }
            None => {
                return Err(ApiError::BadRequest(format!(
                    "Unknown include key '{}'. Valid: {:?}",
                    token,
                    ProfileGroup::all_keys()
                )));
            }
        }
    }
    Ok(out)
}

// Bulk user operations request
#[derive(Debug, Deserialize)]
pub struct BulkUserActionRequest {
    action: String,
    ids: Vec<String>, // UUIDs as strings
    value: Option<String>,
}

/// Perform bulk operations on users (admin only)
pub async fn bulk_users(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    _search_service: web::Data<Arc<SearchService>>,
    body: web::Json<BulkUserActionRequest>,
) -> Result<HttpResponse, ApiError> {
    // Extract claims and check authentication
    let claims = match crate::utils::rbac::require_auth(&req) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(ApiError::Unauthorized(
                "Unauthorized: Authentication required".into(),
            ))
        }
    };

    // Only admins can perform bulk operations
    if !is_platform_admin(&claims) {
        return Err(ApiError::Forbidden(
            "Forbidden: Only administrators can perform bulk user operations".into(),
        ));
    }

    let mut conn = helpers::db_conn(&pool)?;
    // Pin the request workspace so the per-target `user_is_admin` guard resolves
    // the target's role through RLS; on an unpinned conn it reads None and the
    // admin-protection guard fails OPEN.
    helpers::pin_request_workspace(&req, &mut conn);

    let action = body.action.as_str();
    let ids = &body.ids;

    if ids.is_empty() {
        return Err(ApiError::BadRequest(
            "Bad Request: No user IDs provided".into(),
        ));
    }

    // Prevent self-deletion/modification
    if ids.contains(&claims.sub) {
        return Err(ApiError::BadRequest(
            "Bad Request: Cannot perform bulk operations on your own account".into(),
        ));
    }

    match action {
        "delete" => {
            // Bulk soft-delete shares the same primitive as the
            // single-delete handler so the two flows can't drift.
            let actor = helpers::actor_for(&req, "users_admin");
            let mut deleted = 0;
            let mut skipped_admin = 0;
            let mut skipped_staff = 0;
            for id in ids {
                let uuid = match Uuid::parse_str(id) {
                    Ok(u) => u,
                    Err(_) => continue,
                };
                let target = match repository::get_user_by_uuid(&uuid, &mut conn) {
                    Ok(u) => u,
                    Err(_) => continue,
                };
                if crate::repository::user_helpers::user_is_admin(&mut conn, &target) {
                    skipped_admin += 1;
                    continue;
                }
                // In hosted, a staff seat (in any workspace) is control-plane-
                // owned; soft-deleting it would desync the projection. Fails
                // closed on a read error (see the helper).
                if helpers::target_is_externally_managed_staff(&mut conn, &actor, uuid) {
                    skipped_staff += 1;
                    continue;
                }
                if target.deleted_at.is_some() {
                    continue;
                }

                let result = crate::sync::session::with_actor_context::<_, diesel::result::Error>(
                    &mut conn,
                    &actor,
                    |conn| repository::users::soft_delete_user(&uuid, conn),
                );
                match result {
                    Ok(_row) => {
                        deleted += 1;
                    }
                    Err(e) => {
                        error!(user_id = %id, error = ?e, "Error soft-deleting user");
                    }
                }
            }

            Ok(HttpResponse::Ok().json(json!({
                "affected": deleted,
                "skipped_admin": skipped_admin,
                "skipped_staff": skipped_staff,
            })))
        }

        "set-role" => {
            let role_str = match &body.value {
                Some(v) => v.as_str(),
                None => {
                    return Err(ApiError::BadRequest(
                        "Bad Request: Role value required".into(),
                    ))
                }
            };

            let (platform_role_enum, workspace_role_enum) = match utils::parse_roles(role_str) {
                Ok(roles) => roles,
                Err(_) => {
                    return Err(ApiError::BadRequest(
                        "Bad Request: Invalid role value".into(),
                    ))
                }
            };
            let workspace_role = workspace_role_enum.as_str();
            let platform_role = platform_role_enum.as_str();

            // Same actor-context wrapping as the "delete" branch above:
            // the platform_role write hits the audited `users` table, so
            // it needs `app.workspace_id` pinned. The actor's workspace
            // (from the request's WorkspaceContext) also scopes the
            // workspace_members rewrite via the GUC, so the role change
            // lands in the request's workspace under hosted multi-tenancy.
            let actor = helpers::actor_for(&req, "users_admin");
            let ws_id = actor.workspace_id.unwrap_or_default();
            let mut updated = 0;
            let mut skipped_staff = 0;
            for id in ids {
                let uuid = match Uuid::parse_str(id) {
                    Ok(u) => u,
                    Err(_) => continue,
                };

                // set_user_roles (Product authority) rewrites workspace_members.role
                // + platform_role, and refuses a change touching a control-plane-
                // owned staff seat in hosted; those are counted as skipped.
                match crate::sync::session::with_actor_context::<_, diesel::result::Error>(
                    &mut conn,
                    &actor,
                    |c| {
                        repository::users::set_user_roles(
                            c,
                            ws_id,
                            uuid,
                            platform_role,
                            workspace_role,
                            repository::workspaces::SeatWriteAuthority::Product,
                        )
                    },
                ) {
                    Ok(repository::users::SetUserRolesOutcome::Applied) => updated += 1,
                    Ok(repository::users::SetUserRolesOutcome::ExternallyManaged) => {
                        skipped_staff += 1
                    }
                    Err(e) => {
                        error!(user_id = %id, error = ?e, "bulk set-role: update failed; skipping");
                    }
                }
            }

            Ok(HttpResponse::Ok()
                .json(json!({ "affected": updated, "skipped_staff": skipped_staff })))
        }

        _ => Ok(errors::bad_request_with_code(
            format!("Unknown action: {action}"),
            "backend-error-bad-request",
        )),
    }
}

/// Get security info for a user (admin or self)
/// Returns MFA status, passkey list, and auth identities
pub async fn get_user_security_info(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let (claims, _caller_uuid, mut conn) = helpers::auth_conn(&req, &db_pool)?;

    let target_uuid_str = path.into_inner();

    let target_uuid = match utils::parse_uuid(&target_uuid_str) {
        Ok(uuid) => uuid,
        Err(_) => return Err(ApiError::BadRequest("Invalid UUID format".into())),
    };

    // Self, or a workspace admin of the target's own workspace (single-workspace
    // bound; the panel discloses account-global MFA/passkey posture). Platform
    // admins pass. Was self-or-platform, which dead-ended tenant admins trying
    // to load a member's security panel.
    if claims.sub != target_uuid_str {
        helpers::authorize_target_user_action(&req, &db_pool, &claims, target_uuid, true)?;
    }

    let user = match repository::get_user_by_uuid(&target_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => return Err(ApiError::NotFoundMsg("User not found".into())),
    };

    // MFA status. Indexed unused-codes count via
    // `repository::user_recovery_codes::count_unused`.
    let mfa_enabled = user.mfa_enabled;
    let has_backup_codes = repository::user_recovery_codes::count_unused(&mut conn, &user.uuid)
        .map(|n| n > 0)
        .unwrap_or(false);

    // Passkey info
    let passkey_data = match crate::utils::webauthn::load_user_passkey_data(&mut conn, &user.uuid) {
        Ok(data) => data,
        Err(e) => {
            error!(error = ?e, "Failed to load passkeys for user");
            return Err(ApiError::Internal("Failed to load passkeys".into()));
        }
    };
    let passkeys: Vec<serde_json::Value> = passkey_data
        .credentials
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "name": c.name,
                "created_at": c.created_at.to_rfc3339(),
                "last_used_at": c.last_used_at.map(|t| t.to_rfc3339()),
                "transports": c.transports,
                "backup_eligible": c.backup_eligible,
            })
        })
        .collect();

    // Auth identities
    let auth_identities =
        repository::user_auth_identities::get_user_identities_display(&user.uuid, &mut conn)
            .unwrap_or_default();
    let identities_json: Vec<serde_json::Value> = auth_identities
        .iter()
        .map(|i| {
            json!({
                "id": i.id,
                "provider_type": i.provider_type,
                "provider_name": i.provider_name,
                "email": i.email,
                "created_at": i.created_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            })
        })
        .collect();

    Ok(HttpResponse::Ok().json(json!({
        "mfa_enabled": mfa_enabled,
        "has_backup_codes": has_backup_codes,
        "passkey_count": passkeys.len(),
        "passkeys": passkeys,
        "auth_identities": identities_json,
    })))
}

/// Admin-only: reset a user's password
#[derive(Deserialize)]
pub struct AdminResetPasswordRequest {
    pub new_password: String,
}

pub async fn admin_reset_user_password(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<AdminResetPasswordRequest>,
) -> Result<HttpResponse, ApiError> {
    let target_uuid_str = path.into_inner();
    let (claims, user, mut conn) = helpers::admin_user_conn(&req, &db_pool, &target_uuid_str)?;

    // Validate password meets requirements
    let validation = utils::auth::validate_password(&body.new_password);
    if !validation.valid {
        return Err(ApiError::BadRequest(format!(
            "Password validation failed: {}",
            validation.errors.join(", ")
        )));
    }

    let new_hash = match utils::auth::hash_password(&body.new_password) {
        Ok(hash) => hash,
        Err(e) => return Ok(e.into()),
    };

    // Update password hash on the local auth identity
    let rows_updated = match repository::user_auth_identities::update_local_password_hash(
        &mut conn, &user.uuid, &new_hash,
    ) {
        Ok(count) => count,
        // Hosted disables local passwords entirely, so this is a clean "not
        // available here" rather than a server error.
        Err(repository::user_auth_identities::LocalCredentialError::LocalAuthDisabled) => {
            return Ok(errors::local_auth_disabled())
        }
        Err(repository::user_auth_identities::LocalCredentialError::Db(e)) => {
            error!(error = ?e, "Error updating password hash for user");
            return Err(ApiError::Internal("Failed to update password".into()));
        }
    };

    if rows_updated == 0 {
        return Err(ApiError::BadRequest("User does not have a local password identity. Cannot reset password for OAuth-only accounts.".into()));
    }

    // Update password_changed_at on the audited `users` row. The
    // audit trigger requires `app.workspace_id` pinned (NDX01 fires
    // otherwise), so wrap in with_actor_context using the admin's
    // request-derived actor — the workspace pin lands the audit row
    // under the admin's tenant, which is the correct provenance
    // record for the action.
    let now = chrono::Utc::now().naive_utc();
    let actor = helpers::actor_for(&req, "admin_reset_user_password");
    let _ = crate::sync::session::with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &actor,
        |c| {
            repository::users::set_password_changed_at(c, &user.uuid, now)?;
            Ok(())
        },
    );

    info!(admin = %claims.sub, target_user = %target_uuid_str, user_name = %user.name, "Admin reset user password");

    Ok(HttpResponse::Ok().json(json!({
        "status": "success",
        "message": "Password has been reset"
    })))
}

/// Admin-only: disable MFA for a user
pub async fn admin_disable_user_mfa(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let target_uuid_str = path.into_inner();
    let (claims, user, mut conn) = helpers::admin_user_conn(&req, &db_pool, &target_uuid_str)?;

    if !user.mfa_enabled {
        return Err(ApiError::BadRequest(
            "MFA is not enabled for this user".into(),
        ));
    }

    // Codes-first to avoid a window where MFA is off but recovery
    // codes still exist; see `mfa_disable` in handlers/auth.rs.
    if let Err(e) = repository::user_recovery_codes::delete_all_for_user(&mut conn, &user.uuid) {
        error!(error = ?e, "Error clearing recovery codes during admin MFA disable");
        return Err(ApiError::Internal("Failed to disable MFA".into()));
    }
    // Clear both columns: the CHECK constraint requires
    // (mfa_secret IS NULL) = (mfa_secret_kek_id IS NULL).
    let mfa_update = crate::models::UserMfaUpdate {
        mfa_enabled: Some(false),
        mfa_secret: Some(None),
        mfa_secret_kek_id: Some(None),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    if let Err(e) = repository::update_user_mfa(&user.uuid, mfa_update, &mut conn) {
        error!(error = ?e, "Error disabling MFA for user");
        return Err(ApiError::Internal("Failed to disable MFA".into()));
    }

    info!(admin = %claims.sub, target_user = %target_uuid_str, user_name = %user.name, "Admin disabled MFA for user");

    Ok(HttpResponse::Ok().json(json!({
        "status": "success",
        "message": "Two-factor authentication has been disabled"
    })))
}

/// Admin-only: delete a passkey for a user
pub async fn admin_delete_user_passkey(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<(String, String)>, // (user_uuid, credential_id)
) -> Result<HttpResponse, ApiError> {
    let (target_uuid_str, credential_id) = path.into_inner();
    let (claims, user, mut conn) = helpers::admin_user_conn(&req, &db_pool, &target_uuid_str)?;

    match crate::utils::webauthn::delete_credential(&mut conn, &user.uuid, &credential_id) {
        Ok(true) => {}
        Ok(false) => return Err(ApiError::NotFoundMsg("Passkey not found".into())),
        Err(e) => {
            error!(error = ?e, "Error deleting passkey");
            return Err(ApiError::Internal("Failed to delete passkey".into()));
        }
    }

    info!(admin = %claims.sub, target_user = %target_uuid_str, credential_id = %credential_id, "Admin deleted passkey for user");

    Ok(HttpResponse::Ok().json(json!({
        "status": "success",
        "message": "Passkey has been deleted"
    })))
}
