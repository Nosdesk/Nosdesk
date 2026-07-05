//! Ticket CSV importer.
//!
//! Insert-only. Tickets have no natural external key (the
//! database id is system-generated), so this path is for one-
//! shot migrations from another system. Each row creates a
//! ticket; running the same file twice creates duplicates.
//!
//! Columns:
//!   - title           required
//!   - workflow_state  required, must match a workflow_states.name
//!   - priority        optional (low | medium | high), defaults to medium
//!   - requester_email optional, resolved against user_emails
//!   - assignee_email  optional, resolved against user_emails
//!   - category        optional, resolved against ticket_categories.name
//!   - due_date        optional, ISO-8601 date (YYYY-MM-DD)
//!
//! No description column for v1; ticket bodies live in
//! `article_contents` and the import wizard isn't the right
//! surface for collaborative-edit content. Admins fill that in
//! after the bulk create.

use std::collections::{HashMap, HashSet};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewTicket, TicketPriority};
use crate::repository::tickets as ticket_repo;

use super::csv_parser::ParsedCsv;
use super::types::{ImportSummary, ImportedRecords, Importer, RowError, MAX_ERRORS};

const HEADERS: &[&str] = &[
    "title",
    "workflow_state",
    "priority",
    "requester_email",
    "assignee_email",
    "category",
    "due_date",
];

pub struct TicketImporter;

impl Importer for TicketImporter {
    fn template_headers(&self) -> &'static [&'static str] {
        HEADERS
    }

    fn dry_run(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
    ) -> Result<ImportSummary, diesel::result::Error> {
        let mut summary = ImportSummary {
            row_count: parsed.rows.len(),
            would_create: 0,
            would_update: 0,
            errors: Vec::new(),
            errors_truncated: false,
        };

        if let Some(err) = check_headers(&parsed.headers) {
            push_error(&mut summary, 1, None, err);
            return Ok(summary);
        }

        let ctx = ImportContext::load(conn)?;

        for (i, row) in parsed.rows.iter().enumerate() {
            let row_num = i + 2;
            match validate_row(row, &ctx) {
                Ok(_) => summary.would_create += 1,
                Err(errs) => {
                    for (col, msg) in errs {
                        push_error(&mut summary, row_num, col, msg);
                    }
                }
            }
        }
        Ok(summary)
    }

    fn commit(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
    ) -> Result<ImportedRecords, diesel::result::Error> {
        if check_headers(&parsed.headers).is_some() {
            return Err(diesel::result::Error::QueryBuilderError(
                "header validation should have caught this; refusing to commit".into(),
            ));
        }
        let ctx = ImportContext::load(conn)?;

        let mut tickets = Vec::new();
        for row in &parsed.rows {
            let resolved = match validate_row(row, &ctx) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let new = NewTicket {
                title: trimmed(row, "title"),
                workflow_state_id: resolved.workflow_state_id,
                priority: resolved.priority,
                requester_uuid: resolved.requester_uuid,
                assignee_uuid: resolved.assignee_uuid,
                category_id: resolved.category_id,
                submitted_via: Some("csv_import".to_string()),
                guest_lookup_token: None,
                verification_state: None,
                origin_channel_id: None,
                triage_state: None,
                due_date: resolved.due_date,
                start_date: None,
                recurrence_rule: None,
                recurrence_template_id: None,
                resolution_notes: None,
                spam_suspected: false,
            };
            // create_ticket emits the ticket.created sync event in the
            // transaction, so imported tickets reach the activity feed,
            // webhooks, and plugins exactly like API-created ones.
            let ticket = ticket_repo::create_ticket(conn, new)?;
            tickets.push(ticket);
        }
        Ok(ImportedRecords::Tickets(tickets))
    }
}

#[derive(Debug)]
struct ImportContext {
    /// workflow_states.name (case-insensitive key) -> id
    workflow_states: HashMap<String, i32>,
    /// ticket_categories.name (case-insensitive key) -> id
    categories: HashMap<String, i32>,
    /// user_emails (lower-cased) -> user_uuid (primary emails only)
    emails: HashMap<String, Uuid>,
}

impl ImportContext {
    fn load(conn: &mut DbConnection) -> Result<Self, diesel::result::Error> {
        use crate::schema::{ticket_categories, user_emails, workflow_states};

        let ws: Vec<(i32, String)> = workflow_states::table
            .filter(workflow_states::archived_at.is_null())
            .select((workflow_states::id, workflow_states::name))
            .load(conn)?;
        let cats: Vec<(i32, String)> = ticket_categories::table
            .select((ticket_categories::id, ticket_categories::name))
            .load(conn)?;
        let ems: Vec<(String, Uuid)> = user_emails::table
            .filter(user_emails::is_primary.eq(true))
            .select((user_emails::email, user_emails::user_uuid))
            .load(conn)?;

        Ok(Self {
            workflow_states: ws
                .into_iter()
                .map(|(id, n)| (n.to_lowercase(), id))
                .collect(),
            categories: cats
                .into_iter()
                .map(|(id, n)| (n.to_lowercase(), id))
                .collect(),
            emails: ems
                .into_iter()
                .map(|(e, u)| (e.to_lowercase(), u))
                .collect(),
        })
    }
}

struct ResolvedRow {
    workflow_state_id: i32,
    priority: TicketPriority,
    requester_uuid: Option<Uuid>,
    assignee_uuid: Option<Uuid>,
    category_id: Option<i32>,
    due_date: Option<NaiveDateTime>,
}

fn check_headers(headers: &[String]) -> Option<String> {
    let expected: HashSet<&str> = HEADERS.iter().copied().collect();
    let provided: HashSet<&str> = headers.iter().map(String::as_str).collect();
    let missing: Vec<&&str> = expected.difference(&provided).collect();
    if !missing.is_empty() {
        let names: Vec<String> = missing.iter().map(|s| (**s).to_string()).collect();
        return Some(format!("missing required columns: {}", names.join(", ")));
    }
    None
}

fn push_error(summary: &mut ImportSummary, row: usize, column: Option<String>, message: String) {
    if summary.errors.len() >= MAX_ERRORS {
        summary.errors_truncated = true;
        return;
    }
    summary.errors.push(RowError {
        row,
        column,
        message,
    });
}

fn validate_row(
    row: &HashMap<String, String>,
    ctx: &ImportContext,
) -> Result<ResolvedRow, Vec<(Option<String>, String)>> {
    let mut errors: Vec<(Option<String>, String)> = Vec::new();

    let title = trimmed(row, "title");
    if title.is_empty() {
        errors.push((Some("title".into()), "title is required".into()));
    }

    let workflow_state = trimmed(row, "workflow_state").to_lowercase();
    let workflow_state_id = if workflow_state.is_empty() {
        errors.push((
            Some("workflow_state".into()),
            "workflow_state is required".into(),
        ));
        0
    } else {
        match ctx.workflow_states.get(&workflow_state) {
            Some(id) => *id,
            None => {
                errors.push((
                    Some("workflow_state".into()),
                    format!(
                        "unknown workflow state '{}'; create it under Admin -> Workflow first",
                        trimmed(row, "workflow_state")
                    ),
                ));
                0
            }
        }
    };

    let priority = parse_priority(trimmed(row, "priority").as_str()).unwrap_or_else(|| {
        let raw = trimmed(row, "priority");
        if raw.is_empty() {
            TicketPriority::Medium
        } else {
            errors.push((
                Some("priority".into()),
                format!("'{raw}' is not a valid priority; use low, medium, or high"),
            ));
            TicketPriority::Medium
        }
    });

    let requester_uuid = resolve_email(row, "requester_email", ctx, &mut errors);
    let assignee_uuid = resolve_email(row, "assignee_email", ctx, &mut errors);

    let category_id = {
        let raw = trimmed(row, "category");
        if raw.is_empty() {
            None
        } else {
            match ctx.categories.get(&raw.to_lowercase()) {
                Some(id) => Some(*id),
                None => {
                    errors.push((
                        Some("category".into()),
                        format!(
                            "unknown category '{raw}'; create it under Admin -> Categories first"
                        ),
                    ));
                    None
                }
            }
        }
    };

    let due_date = {
        let raw = trimmed(row, "due_date");
        if raw.is_empty() {
            None
        } else {
            match NaiveDate::parse_from_str(&raw, "%Y-%m-%d") {
                Ok(d) => Some(NaiveDateTime::new(
                    d,
                    NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                )),
                Err(_) => {
                    errors.push((
                        Some("due_date".into()),
                        format!("'{raw}' is not a valid date; use YYYY-MM-DD"),
                    ));
                    None
                }
            }
        }
    };

    if errors.is_empty() {
        Ok(ResolvedRow {
            workflow_state_id,
            priority,
            requester_uuid,
            assignee_uuid,
            category_id,
            due_date,
        })
    } else {
        Err(errors)
    }
}

fn resolve_email(
    row: &HashMap<String, String>,
    key: &str,
    ctx: &ImportContext,
    errors: &mut Vec<(Option<String>, String)>,
) -> Option<Uuid> {
    let raw = trimmed(row, key);
    if raw.is_empty() {
        return None;
    }
    match ctx.emails.get(&raw.to_lowercase()) {
        Some(uuid) => Some(*uuid),
        None => {
            errors.push((
                Some(key.to_string()),
                format!("no user has primary email '{raw}'"),
            ));
            None
        }
    }
}

fn parse_priority(s: &str) -> Option<TicketPriority> {
    match s.to_lowercase().as_str() {
        "none" => Some(TicketPriority::None),
        "low" => Some(TicketPriority::Low),
        "medium" => Some(TicketPriority::Medium),
        "high" => Some(TicketPriority::High),
        "urgent" => Some(TicketPriority::Urgent),
        _ => None,
    }
}

fn trimmed(row: &HashMap<String, String>, key: &str) -> String {
    row.get(key).cloned().unwrap_or_default().trim().to_string()
}
