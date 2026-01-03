use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{
    AssignmentMethod, AssignmentRuleUpdate, AssignmentTrigger, Claims, NewAssignmentRule,
};
use crate::repository;
use crate::services::assignment::AssignmentEngine;
use crate::utils::rbac::require_admin;

// ============================================================================
// List Rules
// ============================================================================

/// Get all assignment rules with details (admin only)
pub async fn get_all_rules(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::assignment_rules::get_all_rules_with_details(&mut conn) {
        Ok(rules) => HttpResponse::Ok().json(rules),
        Err(_) => HttpResponse::InternalServerError().json("Failed to get assignment rules"),
    }
}

// ============================================================================
// Get Single Rule
// ============================================================================

/// Get a single rule by ID (admin only)
pub async fn get_rule(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let rule_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::assignment_rules::get_rule_with_details(&mut conn, rule_id) {
        Ok(rule) => HttpResponse::Ok().json(rule),
        Err(e) => match e {
            Error::NotFound => HttpResponse::NotFound().json("Assignment rule not found"),
            _ => HttpResponse::InternalServerError().json("Failed to get assignment rule"),
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

/// Create a new assignment rule (admin only)
pub async fn create_rule(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<CreateAssignmentRuleRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let created_by = Uuid::parse_str(&claims.sub).ok();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Parse method
    let method = match body.method.as_str() {
        "direct_user" => AssignmentMethod::DirectUser,
        "group_round_robin" => AssignmentMethod::GroupRoundRobin,
        "group_random" => AssignmentMethod::GroupRandom,
        "group_queue" => AssignmentMethod::GroupQueue,
        _ => return HttpResponse::BadRequest().json("Invalid assignment method"),
    };

    // Validate method requirements
    match method {
        AssignmentMethod::DirectUser => {
            if body.target_user_uuid.is_none() {
                return HttpResponse::BadRequest()
                    .json("target_user_uuid is required for direct_user method");
            }
        }
        AssignmentMethod::GroupRoundRobin | AssignmentMethod::GroupRandom | AssignmentMethod::GroupQueue => {
            if body.target_group_id.is_none() {
                return HttpResponse::BadRequest()
                    .json("target_group_id is required for group-based methods");
            }
        }
    }

    // Validate conditions JSON size and depth to prevent DoS
    if let Some(ref conditions) = body.conditions {
        let json_str = conditions.to_string();
        if json_str.len() > 10_000 {
            return HttpResponse::BadRequest().json("Conditions JSON too large (max 10KB)");
        }
        // Check nesting depth (simple heuristic: count brackets)
        let depth = json_str.chars().filter(|c| *c == '{' || *c == '[').count();
        if depth > 20 {
            return HttpResponse::BadRequest().json("Conditions JSON too deeply nested");
        }
    }

    // Get next priority if not provided
    let priority = match body.priority {
        Some(p) => p,
        None => repository::assignment_rules::get_next_priority(&mut conn).unwrap_or(100),
    };

    // Check for duplicate name
    if let Ok(true) = repository::assignment_rules::rule_name_exists(&mut conn, &body.name, None) {
        return HttpResponse::Conflict().json("A rule with this name already exists");
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

    match repository::assignment_rules::create_rule(&mut conn, new_rule) {
        Ok(rule) => {
            // Return with full details
            match repository::assignment_rules::get_rule_with_details(&mut conn, rule.id) {
                Ok(details) => HttpResponse::Created().json(details),
                Err(_) => HttpResponse::Created().json(rule),
            }
        }
        Err(_) => HttpResponse::InternalServerError().json("Failed to create assignment rule"),
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

/// Update an assignment rule (admin only)
pub async fn update_rule(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<UpdateAssignmentRuleRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let rule_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Check if rule exists
    let existing = match repository::assignment_rules::get_rule_by_id(&mut conn, rule_id) {
        Ok(r) => r,
        Err(Error::NotFound) => return HttpResponse::NotFound().json("Assignment rule not found"),
        Err(_) => return HttpResponse::InternalServerError().json("Database error"),
    };

    // Parse method if provided
    let method = match &body.method {
        Some(m) => match m.as_str() {
            "direct_user" => Some(AssignmentMethod::DirectUser),
            "group_round_robin" => Some(AssignmentMethod::GroupRoundRobin),
            "group_random" => Some(AssignmentMethod::GroupRandom),
            "group_queue" => Some(AssignmentMethod::GroupQueue),
            _ => return HttpResponse::BadRequest().json("Invalid assignment method"),
        },
        None => None,
    };

    // Check for duplicate name if name is being changed
    if let Some(ref new_name) = body.name {
        if new_name != &existing.name {
            if let Ok(true) = repository::assignment_rules::rule_name_exists(&mut conn, new_name, Some(rule_id)) {
                return HttpResponse::Conflict().json("A rule with this name already exists");
            }
        }
    }

    // Validate conditions JSON size and depth to prevent DoS
    if let Some(ref conditions) = body.conditions {
        let json_str = conditions.to_string();
        if json_str.len() > 10_000 {
            return HttpResponse::BadRequest().json("Conditions JSON too large (max 10KB)");
        }
        let depth = json_str.chars().filter(|c| *c == '{' || *c == '[').count();
        if depth > 20 {
            return HttpResponse::BadRequest().json("Conditions JSON too deeply nested");
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

    match repository::assignment_rules::update_rule(&mut conn, rule_id, rule_update) {
        Ok(_) => {
            // Return with full details
            match repository::assignment_rules::get_rule_with_details(&mut conn, rule_id) {
                Ok(details) => HttpResponse::Ok().json(details),
                Err(_) => HttpResponse::InternalServerError().json("Failed to get updated rule"),
            }
        }
        Err(Error::NotFound) => HttpResponse::NotFound().json("Assignment rule not found"),
        Err(_) => HttpResponse::InternalServerError().json("Failed to update assignment rule"),
    }
}

// ============================================================================
// Delete Rule
// ============================================================================

/// Delete an assignment rule (admin only)
pub async fn delete_rule(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let rule_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::assignment_rules::delete_rule(&mut conn, rule_id) {
        Ok(0) => HttpResponse::NotFound().json("Assignment rule not found"),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(_) => HttpResponse::InternalServerError().json("Failed to delete assignment rule"),
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
    pool: web::Data<Pool>,
    body: web::Json<ReorderRulesRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let orders: Vec<(i32, i32)> = body.orders.iter().map(|o| (o.id, o.priority)).collect();

    match repository::assignment_rules::reorder_rules(&mut conn, orders) {
        Ok(_) => {
            // Return all rules with updated order
            match repository::assignment_rules::get_all_rules_with_details(&mut conn) {
                Ok(rules) => HttpResponse::Ok().json(rules),
                Err(_) => HttpResponse::InternalServerError().json("Failed to get updated rules"),
            }
        }
        Err(_) => HttpResponse::InternalServerError().json("Failed to reorder rules"),
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

/// Preview what assignment would happen for a ticket (admin only)
pub async fn preview_assignment(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<PreviewAssignmentRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Parse trigger
    let trigger = match body.trigger.as_str() {
        "ticket_created" => AssignmentTrigger::TicketCreated,
        "category_changed" => AssignmentTrigger::CategoryChanged,
        _ => return HttpResponse::BadRequest().json("Invalid trigger type"),
    };

    // Get the ticket
    let ticket = match repository::get_ticket_by_id(&mut conn, body.ticket_id) {
        Ok(t) => t,
        Err(_) => return HttpResponse::NotFound().json("Ticket not found"),
    };

    // Check if ticket already has assignee
    if ticket.assignee_uuid.is_some() {
        return HttpResponse::Ok().json(PreviewAssignmentResponse {
            would_assign: false,
            rule_id: None,
            rule_name: None,
            assigned_user_uuid: None,
            method: None,
            message: "Ticket already has an assignee".to_string(),
        });
    }

    // Evaluate rules
    match AssignmentEngine::evaluate_rules(&mut conn, &ticket, trigger) {
        Some(result) => HttpResponse::Ok().json(PreviewAssignmentResponse {
            would_assign: true,
            rule_id: Some(result.rule_id),
            rule_name: Some(result.rule_name),
            assigned_user_uuid: result.assigned_user_uuid,
            method: Some(result.method.to_string()),
            message: "Assignment would be made".to_string(),
        }),
        None => HttpResponse::Ok().json(PreviewAssignmentResponse {
            would_assign: false,
            rule_id: None,
            rule_name: None,
            assigned_user_uuid: None,
            method: None,
            message: "No matching assignment rule found".to_string(),
        }),
    }
}

// ============================================================================
// Get Assignment Logs
// ============================================================================

/// Get recent assignment logs (admin only)
pub async fn get_assignment_logs(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::assignment_rules::get_recent_logs(&mut conn, 100) {
        Ok(logs) => HttpResponse::Ok().json(logs),
        Err(_) => HttpResponse::InternalServerError().json("Failed to get assignment logs"),
    }
}
