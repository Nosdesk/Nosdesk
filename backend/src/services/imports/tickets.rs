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

use chrono::{NaiveDateTime, NaiveTime};
use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewTicket, TicketPriority};
use crate::repository::tickets as ticket_repo;

use super::csv_parser::ParsedCsv;
use super::types::{
    CommitError, Committed, FieldGroup, FieldSpec, ImportContext, ImportType, ImportedRecords,
    Importer, RecordId, RowOutcome, RowPlan, RowResult, ValueType,
};
use super::values::{self, Choice};

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

    fn fields(&self, _conn: &mut DbConnection) -> Result<Vec<FieldSpec>, diesel::result::Error> {
        use FieldGroup::{Core, Links};
        let t = ImportType::Tickets;
        Ok(vec![
            FieldSpec::fixed(t, "title", "Title", Core, ValueType::Text, true),
            FieldSpec::fixed(t, "workflow_state", "Status", Core, ValueType::Text, true),
            FieldSpec::fixed(t, "priority", "Priority", Core, ValueType::Enum, false).with_enum(
                TicketPriority::ALL
                    .iter()
                    .map(|p| p.as_str().to_string())
                    .collect(),
            ),
            FieldSpec::fixed(t, "due_date", "Due date", Core, ValueType::Date, false)
                .with_format("YYYY-MM-DD"),
            FieldSpec::fixed(
                t,
                "requester_email",
                "Requester email",
                Links,
                ValueType::User,
                false,
            ),
            FieldSpec::fixed(
                t,
                "assignee_email",
                "Assignee email",
                Links,
                ValueType::User,
                false,
            ),
            FieldSpec::fixed(t, "category", "Category", Links, ValueType::Text, false),
        ])
    }

    fn synonyms(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("subject", "title"),
            ("summary", "title"),
            ("state", "workflow_state"),
            ("workflowstate", "workflow_state"),
            ("requester", "requester_email"),
            ("requestedby", "requester_email"),
            ("reporter", "requester_email"),
            ("assignee", "assignee_email"),
            ("assignedto", "assignee_email"),
            ("due", "due_date"),
        ]
    }

    fn value_choices(
        &self,
        conn: &mut DbConnection,
        field: &FieldSpec,
    ) -> Result<Option<Vec<Choice>>, diesel::result::Error> {
        use crate::schema::workflow_states;
        match field.id.as_str() {
            "workflow_state" => {
                let names: Vec<String> = workflow_states::table
                    .filter(workflow_states::archived_at.is_null())
                    .order(workflow_states::name.asc())
                    .select(workflow_states::name)
                    .load(conn)?;
                Ok(Some(
                    names
                        .into_iter()
                        .map(|n| Choice::new(n.clone(), n))
                        .collect(),
                ))
            }
            "priority" => Ok(Some(
                TicketPriority::ALL
                    .iter()
                    .map(|p| Choice::new(p.as_str(), priority_label(*p)))
                    .collect(),
            )),
            _ => Ok(None),
        }
    }

    fn value_synonyms(&self, field: &str) -> &'static [(&'static str, &'static str)] {
        match field {
            "priority" => &[
                ("normal", "medium"),
                ("med", "medium"),
                ("critical", "urgent"),
                ("highest", "urgent"),
                ("lowest", "low"),
            ],
            _ => &[],
        }
    }

    fn dry_run(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        _ctx: &ImportContext,
    ) -> Result<RowPlan, diesel::result::Error> {
        if let Some(err) = check_headers(&parsed.headers) {
            return Ok(RowPlan {
                file_error: Some(err),
                ..Default::default()
            });
        }

        let lookups = Lookups::load(conn)?;
        let mut plan = RowPlan::default();

        for record in parsed.records() {
            let row_num = record.source_row;
            let result = match validate_row(&record.values, &lookups) {
                Ok(_) => RowResult::planned(row_num, RowOutcome::Create),
                Err(errs) => RowResult::skipped(row_num, errs),
            };
            plan.rows
                .push(result.with_label(trimmed(&record.values, "title")));
        }
        Ok(plan)
    }

    fn commit(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        _ctx: &ImportContext,
    ) -> Result<Committed, CommitError> {
        if check_headers(&parsed.headers).is_some() {
            return Err(diesel::result::Error::QueryBuilderError(
                "header validation should have caught this; refusing to commit".into(),
            )
            .into());
        }
        let lookups = Lookups::load(conn)?;

        let mut tickets = Vec::new();
        let mut rows = Vec::with_capacity(parsed.rows.len());
        for record in parsed.records() {
            let row = &record.values;
            let title = trimmed(row, "title");
            let resolved = match validate_row(row, &lookups) {
                Ok(r) => r,
                Err(errs) => {
                    rows.push(RowResult::skipped(record.source_row, errs).with_label(title));
                    continue;
                }
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
            rows.push(
                RowResult::planned(record.source_row, RowOutcome::Create)
                    .with_label(title)
                    .with_record(RecordId::Int(ticket.id)),
            );
            tickets.push(ticket);
        }
        Ok(Committed {
            records: ImportedRecords::Tickets(tickets),
            rows,
        })
    }
}

#[derive(Debug)]
struct Lookups {
    /// workflow_states.name (case-insensitive key) -> id
    workflow_states: HashMap<String, i32>,
    /// ticket_categories.name (case-insensitive key) -> id
    categories: HashMap<String, i32>,
    /// user_emails (lower-cased) -> user_uuid (primary emails only)
    emails: HashMap<String, Uuid>,
    /// Of those users, the ones who can be assigned tickets here.
    assignable: HashSet<Uuid>,
}

impl Lookups {
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

        let users: Vec<Uuid> = ems.iter().map(|(_, u)| *u).collect();
        let assignable = crate::repository::assignees::assignable_among(conn, &users)?;

        Ok(Self {
            assignable,
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

fn validate_row(
    row: &HashMap<String, String>,
    lookups: &Lookups,
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
        match lookups.workflow_states.get(&workflow_state) {
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
                format!("'{raw}' is not a valid priority; use none, low, medium, high or urgent"),
            ));
            TicketPriority::Medium
        }
    });

    let requester_uuid = resolve_email(row, "requester_email", lookups, &mut errors);
    let assignee_uuid = resolve_email(row, "assignee_email", lookups, &mut errors);
    if assignee_uuid.is_some_and(|u| !lookups.assignable.contains(&u)) {
        errors.push((
            Some("assignee_email".into()),
            format!(
                "'{}' can't be assigned tickets; only agents and admins can",
                trimmed(row, "assignee_email")
            ),
        ));
    }

    let category_id = {
        let raw = trimmed(row, "category");
        if raw.is_empty() {
            None
        } else {
            match lookups.categories.get(&raw.to_lowercase()) {
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
            match values::parse_date(&raw) {
                Some(d) => Some(NaiveDateTime::new(
                    d,
                    NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                )),
                None => {
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
    lookups: &Lookups,
    errors: &mut Vec<(Option<String>, String)>,
) -> Option<Uuid> {
    let raw = trimmed(row, key);
    if raw.is_empty() {
        return None;
    }
    match lookups.emails.get(&raw.to_lowercase()) {
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

/// A priority as the ticket page names it.
fn priority_label(p: TicketPriority) -> &'static str {
    match p {
        TicketPriority::None => "None",
        TicketPriority::Low => "Low",
        TicketPriority::Medium => "Medium",
        TicketPriority::High => "High",
        TicketPriority::Urgent => "Urgent",
    }
}

fn parse_priority(s: &str) -> Option<TicketPriority> {
    TicketPriority::parse(s)
}

fn trimmed(row: &HashMap<String, String>, key: &str) -> String {
    row.get(key).cloned().unwrap_or_default().trim().to_string()
}
