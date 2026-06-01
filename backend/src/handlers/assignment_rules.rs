use actix_web::{web, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::models::{
    AssignmentMethod, AssignmentRuleUpdate, AssignmentTrigger, NewAssignmentRule, WorkspaceRole,
};
use crate::repository;
use crate::services::assignment::AssignmentEngine;
use crate::utils::rbac::require_workspace_role;

// ============================================================================
// List Rules
// ============================================================================

/// Get all assignment rules with details (admin only)
pub async fn get_all_rules(req: HttpRequest, mut tc: TenantConn) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    match tc.run(|conn| repository::assignment_rules::get_all_rules_with_details(conn)) {
        Ok(rules) => HttpResponse::Ok().json(rules),
        Err(_) => errors::internal("Failed to get assignment rules"),
    }
}

// ============================================================================
// Get Single Rule
// ============================================================================

/// Get a single rule by ID (admin only)
pub async fn get_rule(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let rule_id = path.into_inner();
    match tc.run(|conn| repository::assignment_rules::get_rule_with_details(conn, rule_id)) {
        Ok(rule) => HttpResponse::Ok().json(rule),
        Err(e) => match e {
            Error::NotFound => errors::not_found_msg("Assignment rule not found"),
            _ => errors::internal("Failed to get assignment rule"),
        },
    }
}

// ============================================================================
// Create Rule
// ============================================================================

/// Request body for creating an assignment rule
#[derive(Debug, Deserialize)]
pub struct CreateAssignmentRuleRequest {
    pub name: String,
    pub description: Option<String>,
    pub priority: Option<i32>,
    pub is_active: Option<bool>,
    pub method: String, // "direct_user", "group_round_robin", "group_random", "group_queue"
    pub target_user_uuid: Option<Uuid>,
    pub target_group_id: Option<i32>,
    pub trigger_on_create: Option<bool>,
    pub trigger_on_category_change: Option<bool>,
    pub category_id: Option<i32>,
    pub conditions: Option<Value>,
}

/// Result variants for the create-rule flow so we can pull HTTP
/// distinctions (conflict, bad-request returned via repo errors)
/// back out of the transaction.
enum CreateOutcome {
    Created(serde_json::Value),
    Conflict,
}

/// Create a new assignment rule (admin only)
pub async fn create_rule(
    req: HttpRequest,
    mut tc: TenantConn,
    auth: AuthContext,
    body: web::Json<CreateAssignmentRuleRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let created_by = Some(auth.user_uuid);

    // Parse method
    let method = match body.method.as_str() {
        "direct_user" => AssignmentMethod::DirectUser,
        "group_round_robin" => AssignmentMethod::GroupRoundRobin,
        "group_random" => AssignmentMethod::GroupRandom,
        "group_queue" => AssignmentMethod::GroupQueue,
        _ => return errors::bad_request("Invalid assignment method"),
    };

    // Validate method requirements
    match method {
        AssignmentMethod::DirectUser => {
            if body.target_user_uuid.is_none() {
                return errors::bad_request("target_user_uuid is required for direct_user method");
            }
        }
        AssignmentMethod::GroupRoundRobin
        | AssignmentMethod::GroupRandom
        | AssignmentMethod::GroupQueue => {
            if body.target_group_id.is_none() {
                return errors::bad_request("target_group_id is required for group-based methods");
            }
        }
    }

    // Validate conditions JSON size and depth to prevent DoS
    if let Some(ref conditions) = body.conditions {
        let json_str = conditions.to_string();
        if json_str.len() > 10_000 {
            return errors::bad_request("Conditions JSON too large (max 10KB)");
        }
        // Check nesting depth (simple heuristic: count brackets)
        let depth = json_str.chars().filter(|c| *c == '{' || *c == '[').count();
        if depth > 20 {
            return errors::bad_request("Conditions JSON too deeply nested");
        }
    }

    let body = body.into_inner();

    let result = tc.run(|conn| {
        // Get next priority if not provided
        let priority = match body.priority {
            Some(p) => p,
            None => repository::assignment_rules::get_next_priority(conn).unwrap_or(100),
        };

        // Check for duplicate name
        if let Ok(true) = repository::assignment_rules::rule_name_exists(conn, &body.name, None) {
            return Ok(CreateOutcome::Conflict);
        }

        let new_rule = NewAssignmentRule {
            name: body.name.clone(),
            description: body.description.clone(),
            priority,
            is_active: body.is_active.unwrap_or(true),
            method,
            target_user_uuid: body.target_user_uuid,
            target_group_id: body.target_group_id,
            trigger_on_create: body.trigger_on_create.unwrap_or(true),
            trigger_on_category_change: body.trigger_on_category_change.unwrap_or(true),
            category_id: body.category_id,
            conditions: body.conditions.clone(),
            created_by,
        };

        let rule = repository::assignment_rules::create_rule(conn, new_rule)?;
        // Return with full details
        let body = match repository::assignment_rules::get_rule_with_details(conn, rule.id) {
            Ok(details) => serde_json::to_value(details).unwrap_or(serde_json::Value::Null),
            Err(_) => serde_json::to_value(rule).unwrap_or(serde_json::Value::Null),
        };
        Ok(CreateOutcome::Created(body))
    });

    match result {
        Ok(CreateOutcome::Created(body)) => HttpResponse::Created().json(body),
        Ok(CreateOutcome::Conflict) => errors::conflict("A rule with this name already exists"),
        Err(_) => errors::internal("Failed to create assignment rule"),
    }
}

// ============================================================================
// Update Rule
// ============================================================================

/// Request body for updating an assignment rule
#[derive(Debug, Deserialize)]
pub struct UpdateAssignmentRuleRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i32>,
    pub is_active: Option<bool>,
    pub method: Option<String>,
    pub target_user_uuid: Option<Option<Uuid>>,
    pub target_group_id: Option<Option<i32>>,
    pub trigger_on_create: Option<bool>,
    pub trigger_on_category_change: Option<bool>,
    pub category_id: Option<Option<i32>>,
    pub conditions: Option<Value>,
}

/// Result variants for update-rule, mirroring `CreateOutcome`.
enum UpdateOutcome {
    Ok(serde_json::Value),
    NotFound,
    Conflict,
}

/// Update an assignment rule (admin only)
pub async fn update_rule(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<UpdateAssignmentRuleRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let rule_id = path.into_inner();

    // Parse method if provided
    let method = match &body.method {
        Some(m) => match m.as_str() {
            "direct_user" => Some(AssignmentMethod::DirectUser),
            "group_round_robin" => Some(AssignmentMethod::GroupRoundRobin),
            "group_random" => Some(AssignmentMethod::GroupRandom),
            "group_queue" => Some(AssignmentMethod::GroupQueue),
            _ => return errors::bad_request("Invalid assignment method"),
        },
        None => None,
    };

    // Validate conditions JSON size and depth to prevent DoS
    if let Some(ref conditions) = body.conditions {
        let json_str = conditions.to_string();
        if json_str.len() > 10_000 {
            return errors::bad_request("Conditions JSON too large (max 10KB)");
        }
        let depth = json_str.chars().filter(|c| *c == '{' || *c == '[').count();
        if depth > 20 {
            return errors::bad_request("Conditions JSON too deeply nested");
        }
    }

    let body = body.into_inner();

    let result = tc.run(|conn| {
        // Check if rule exists
        let existing = match repository::assignment_rules::get_rule_by_id(conn, rule_id) {
            Ok(r) => r,
            Err(Error::NotFound) => return Ok(UpdateOutcome::NotFound),
            Err(e) => return Err(e),
        };

        // Check for duplicate name if name is being changed
        if let Some(ref new_name) = body.name {
            if new_name != &existing.name {
                if let Ok(true) =
                    repository::assignment_rules::rule_name_exists(conn, new_name, Some(rule_id))
                {
                    return Ok(UpdateOutcome::Conflict);
                }
            }
        }

        let rule_update = AssignmentRuleUpdate {
            name: body.name.clone(),
            description: body.description.clone(),
            priority: body.priority,
            is_active: body.is_active,
            method,
            target_user_uuid: body.target_user_uuid,
            target_group_id: body.target_group_id,
            trigger_on_create: body.trigger_on_create,
            trigger_on_category_change: body.trigger_on_category_change,
            category_id: body.category_id,
            conditions: body.conditions.clone(),
            updated_at: None,
        };

        match repository::assignment_rules::update_rule(conn, rule_id, rule_update) {
            Ok(_) => {
                let details = repository::assignment_rules::get_rule_with_details(conn, rule_id)?;
                Ok(UpdateOutcome::Ok(
                    serde_json::to_value(details).unwrap_or(serde_json::Value::Null),
                ))
            }
            Err(Error::NotFound) => Ok(UpdateOutcome::NotFound),
            Err(e) => Err(e),
        }
    });

    match result {
        Ok(UpdateOutcome::Ok(body)) => HttpResponse::Ok().json(body),
        Ok(UpdateOutcome::NotFound) => errors::not_found_msg("Assignment rule not found"),
        Ok(UpdateOutcome::Conflict) => errors::conflict("A rule with this name already exists"),
        Err(_) => errors::internal("Failed to update assignment rule"),
    }
}

// ============================================================================
// Delete Rule
// ============================================================================

/// Delete an assignment rule (admin only)
pub async fn delete_rule(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let rule_id = path.into_inner();
    match tc.run(|conn| repository::assignment_rules::delete_rule(conn, rule_id)) {
        Ok(0) => errors::not_found_msg("Assignment rule not found"),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(_) => errors::internal("Failed to delete assignment rule"),
    }
}

// ============================================================================
// Reorder Rules
// ============================================================================

/// Request body for reordering rules
#[derive(Debug, Deserialize)]
pub struct ReorderRulesRequest {
    pub orders: Vec<RuleOrder>,
}

#[derive(Debug, Deserialize)]
pub struct RuleOrder {
    pub id: i32,
    pub priority: i32,
}

/// Reorder rules by priority (admin only)
pub async fn reorder_rules(
    req: HttpRequest,
    mut tc: TenantConn,
    body: web::Json<ReorderRulesRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let orders: Vec<(i32, i32)> = body.orders.iter().map(|o| (o.id, o.priority)).collect();

    let result = tc.run(|conn| {
        repository::assignment_rules::reorder_rules(conn, orders)?;
        repository::assignment_rules::get_all_rules_with_details(conn)
    });
    match result {
        Ok(rules) => HttpResponse::Ok().json(rules),
        Err(_) => errors::internal("Failed to reorder rules"),
    }
}

// ============================================================================
// Preview Assignment
// ============================================================================

/// Request body for previewing assignment
#[derive(Debug, Deserialize)]
pub struct PreviewAssignmentRequest {
    pub ticket_id: i32,
    pub trigger: String, // "ticket_created" or "category_changed"
}

/// Response for assignment preview
#[derive(Debug, Serialize)]
pub struct PreviewAssignmentResponse {
    pub would_assign: bool,
    pub rule_id: Option<i32>,
    pub rule_name: Option<String>,
    pub assigned_user_uuid: Option<Uuid>,
    pub method: Option<String>,
    pub message: String,
}

/// Result variants for the preview flow.
enum PreviewOutcome {
    Assigned(PreviewAssignmentResponse),
    NoMatch,
    AlreadyAssigned,
    TicketNotFound,
}

/// Preview what assignment would happen for a ticket (admin only)
pub async fn preview_assignment(
    req: HttpRequest,
    mut tc: TenantConn,
    body: web::Json<PreviewAssignmentRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    // Parse trigger
    let trigger = match body.trigger.as_str() {
        "ticket_created" => AssignmentTrigger::TicketCreated,
        "category_changed" => AssignmentTrigger::CategoryChanged,
        _ => return errors::bad_request("Invalid trigger type"),
    };

    let ticket_id = body.ticket_id;

    let result = tc.run(|conn| {
        // Get the ticket
        let ticket = match repository::get_ticket_by_id(conn, ticket_id) {
            Ok(t) => t,
            Err(Error::NotFound) => return Ok(PreviewOutcome::TicketNotFound),
            Err(e) => return Err(e),
        };

        // Check if ticket already has assignee
        if ticket.assignee_uuid.is_some() {
            return Ok(PreviewOutcome::AlreadyAssigned);
        }

        // Evaluate rules — same call site as the production handlers,
        // already wired through TenantConn there.
        Ok::<_, diesel::result::Error>(
            match AssignmentEngine::evaluate_rules(conn, &ticket, trigger) {
                Some(eval) => PreviewOutcome::Assigned(PreviewAssignmentResponse {
                    would_assign: true,
                    rule_id: Some(eval.rule_id),
                    rule_name: Some(eval.rule_name),
                    assigned_user_uuid: eval.assigned_user_uuid,
                    method: Some(eval.method.to_string()),
                    message: "Assignment would be made".to_string(),
                }),
                None => PreviewOutcome::NoMatch,
            },
        )
    });

    match result {
        Ok(PreviewOutcome::Assigned(resp)) => HttpResponse::Ok().json(resp),
        Ok(PreviewOutcome::NoMatch) => HttpResponse::Ok().json(PreviewAssignmentResponse {
            would_assign: false,
            rule_id: None,
            rule_name: None,
            assigned_user_uuid: None,
            method: None,
            message: "No matching assignment rule found".to_string(),
        }),
        Ok(PreviewOutcome::AlreadyAssigned) => HttpResponse::Ok().json(PreviewAssignmentResponse {
            would_assign: false,
            rule_id: None,
            rule_name: None,
            assigned_user_uuid: None,
            method: None,
            message: "Ticket already has an assignee".to_string(),
        }),
        Ok(PreviewOutcome::TicketNotFound) => errors::not_found_msg("Ticket not found"),
        Err(_) => errors::internal("Failed to preview assignment"),
    }
}

// ============================================================================
// Get Assignment Logs
// ============================================================================

/// Get recent assignment logs (admin only)
pub async fn get_assignment_logs(req: HttpRequest, mut tc: TenantConn) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    match tc.run(|conn| repository::assignment_rules::get_recent_logs(conn, 100)) {
        Ok(logs) => HttpResponse::Ok().json(logs),
        Err(_) => errors::internal("Failed to get assignment logs"),
    }
}

#[cfg(test)]
mod tests {
    //! Permission-boundary tests. Auto-routing rules can silently
    //! redirect tickets between groups, so the gate matters.
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{claims_for, setup_test_pool};
    use actix_web::test as actix_test;
    use actix_web::{http::StatusCode, App, HttpMessage};

    fn test_app(
        pool: crate::db::Pool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(web::Data::new(pool))
            .route("/admin/assignment-rules", web::get().to(get_all_rules))
            .route(
                "/admin/assignment-rules/{id}",
                web::delete().to(delete_rule),
            )
    }

    #[actix_web::test]
    async fn list_requires_authentication() {
        let pool = setup_test_pool();
        let app = actix_test::init_service(test_app(pool)).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/assignment-rules")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn list_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, UserRole::User);
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/assignment-rules")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn delete_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, UserRole::User);
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::delete()
            .uri("/admin/assignment-rules/1")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
