//! Zendesk records to bundle records. Pure functions, no I/O: the API path
//! and the export path both come through here, so a ticket reads the same
//! whichever way it was fetched.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};

use super::api;
use crate::services::imports::helpdesk::bundle::{
    Attachment, Body, BodyFormat, Comment, Person, PersonRole, PriorityLevel, StatusKind, Ticket,
};

/// Organisation id → name, so a person carries their organisation by name.
pub type Organizations = HashMap<i64, String>;

pub fn organizations(orgs: impl IntoIterator<Item = api::Organization>) -> Organizations {
    orgs.into_iter()
        .filter_map(|o| {
            let name = o.name?.trim().to_string();
            (!name.is_empty()).then_some((o.id, name))
        })
        .collect()
}

pub fn person(u: &api::User, orgs: &Organizations) -> Person {
    let role = match u.role.as_deref() {
        Some("admin") => PersonRole::Admin,
        Some("agent") => PersonRole::Agent,
        // "end-user", and anything Zendesk adds later: the least access.
        _ => PersonRole::Requester,
    };
    let email = u
        .email
        .as_deref()
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty());
    let name = u
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .or_else(|| email.clone())
        .unwrap_or_else(|| format!("Zendesk user {}", u.id));
    Person {
        source_id: u.id.to_string(),
        email,
        name,
        role,
        active: u.active && !u.suspended,
        organization: u.organization_id.and_then(|id| orgs.get(&id).cloned()),
        created_at: u.created_at,
        updated_at: u.updated_at,
    }
}

/// Custom ticket fields by id: the title people know them by, and for
/// dropdowns the label behind each stored tag value.
#[derive(Debug, Default)]
pub struct FieldCatalog {
    fields: HashMap<i64, (String, HashMap<String, String>)>,
}

impl FieldCatalog {
    pub fn new(fields: impl IntoIterator<Item = api::TicketField>) -> Self {
        let fields = fields
            .into_iter()
            .map(|f| {
                let title = f
                    .title
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| format!("Field {}", f.id));
                let options = f
                    .custom_field_options
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|o| Some((o.value?, o.name?)))
                    .collect();
                (f.id, (title, options))
            })
            .collect();
        FieldCatalog { fields }
    }

    fn title(&self, id: i64) -> String {
        self.fields
            .get(&id)
            .map(|(t, _)| t.clone())
            .unwrap_or_else(|| format!("Field {id}"))
    }

    fn label(&self, id: i64, value: &str) -> String {
        self.fields
            .get(&id)
            .and_then(|(_, opts)| opts.get(value))
            .cloned()
            .unwrap_or_else(|| value.to_string())
    }

    /// A custom field's value as text, or None when it holds nothing worth
    /// a line in the note. An unticked checkbox is `false` on every ticket,
    /// so it counts as nothing.
    fn render(&self, id: i64, value: &serde_json::Value) -> Option<String> {
        use serde_json::Value;
        let text = match value {
            Value::Null | Value::Bool(false) => return None,
            Value::Bool(true) => "Yes".to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => self.label(id, s.trim()),
            Value::Array(items) => items
                .iter()
                .filter_map(|v| match v {
                    Value::String(s) if !s.trim().is_empty() => Some(self.label(id, s.trim())),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(", "),
            Value::Object(_) => value.to_string(),
        };
        let text = text.trim().to_string();
        (!text.is_empty()).then_some(text)
    }
}

pub fn status_kind(status: &str) -> StatusKind {
    match status {
        "new" => StatusKind::New,
        "pending" => StatusKind::Pending,
        "hold" => StatusKind::OnHold,
        "solved" => StatusKind::Solved,
        "closed" => StatusKind::Closed,
        // "open", and custom statuses (which Zendesk still reports under
        // one of the six categories, so this is a fallback only).
        _ => StatusKind::Open,
    }
}

pub fn priority(p: &str) -> Option<PriorityLevel> {
    match p {
        "low" => Some(PriorityLevel::Low),
        "normal" => Some(PriorityLevel::Normal),
        "high" => Some(PriorityLevel::High),
        "urgent" => Some(PriorityLevel::Urgent),
        _ => None,
    }
}

/// A Zendesk comment's text: the HTML Zendesk rendered when it has one,
/// otherwise the plain text.
pub fn body(c: &api::Comment) -> Body {
    if let Some(html) = c.html_body.as_ref().filter(|h| !h.trim().is_empty()) {
        return Body {
            format: BodyFormat::Html,
            value: html.clone(),
        };
    }
    let plain = c
        .plain_body
        .as_ref()
        .filter(|p| !p.trim().is_empty())
        .or(c.body.as_ref())
        .cloned()
        .unwrap_or_default();
    Body {
        format: BodyFormat::Plain,
        value: plain,
    }
}

/// True when a comment's HTML shows an image hosted by Zendesk. The loader
/// deals with those; the connector only says they exist.
pub fn has_zendesk_inline_image(body: &Body) -> bool {
    if body.format != BodyFormat::Html {
        return false;
    }
    let lower = body.value.to_ascii_lowercase();
    lower.contains("<img") && (lower.contains("zendesk.com") || lower.contains("zdusercontent.com"))
}

/// A comment without its attachments; the caller adds those once it knows
/// which were stored.
pub fn comment(
    c: &api::Comment,
    fallback_at: DateTime<Utc>,
    attachments: Vec<Attachment>,
) -> Comment {
    Comment {
        source_id: c.id.to_string(),
        author: c.author_id.map(|id| id.to_string()),
        public: c.public,
        body: body(c),
        created_at: c.created_at.unwrap_or(fallback_at),
        attachments,
    }
}

/// True when Zendesk says the ticket is gone. `exclude_deleted` should keep
/// these out, but one that slips through is not imported.
pub fn is_deleted(t: &api::Ticket) -> bool {
    t.status.as_deref() == Some("deleted")
}

/// When a ticket was raised: its own timestamp, falling back to the last
/// update. None when Zendesk gave neither, which no real ticket does.
pub fn ticket_created_at(t: &api::Ticket) -> Option<DateTime<Utc>> {
    t.created_at.or(t.updated_at)
}

/// The bundle ticket for a Zendesk ticket and its already-mapped comments.
/// `comments` are sorted oldest first here, whatever order they came in.
pub fn ticket(
    t: &api::Ticket,
    created_at: DateTime<Utc>,
    fields: &FieldCatalog,
    mut comments: Vec<Comment>,
) -> Ticket {
    comments.sort_by(|a, b| {
        a.created_at.cmp(&b.created_at).then_with(|| {
            a.source_id
                .parse::<i64>()
                .ok()
                .cmp(&b.source_id.parse::<i64>().ok())
        })
    });

    let requester = t.requester_id;
    let mut seen = HashSet::new();
    let watchers = t
        .collaborator_ids
        .iter()
        .chain(&t.follower_ids)
        .chain(&t.email_cc_ids)
        .filter(|id| Some(**id) != requester && seen.insert(**id))
        .map(|id| id.to_string())
        .collect();

    let field_values: BTreeMap<String, String> = t
        .custom_fields
        .iter()
        .filter_map(|f| Some((fields.title(f.id), fields.render(f.id, &f.value)?)))
        .collect();

    let status_label = t.status.clone().unwrap_or_else(|| "open".to_string());
    let subject = t
        .subject
        .as_deref()
        .or(t.raw_subject.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Zendesk ticket #{}", t.id));

    Ticket {
        source_id: t.id.to_string(),
        number: Some(t.id),
        subject,
        status: status_kind(&status_label),
        status_label,
        priority: t.priority.as_deref().and_then(priority),
        ticket_type: t.ticket_type.clone().filter(|s| !s.is_empty()),
        tags: t.tags.clone(),
        requester: requester.map(|id| id.to_string()),
        assignee: t.assignee_id.map(|id| id.to_string()),
        watchers,
        created_at,
        updated_at: t.updated_at.unwrap_or(created_at),
        // Zendesk keeps solved time in ticket metrics, one request per
        // ticket; not worth it for a field the loader can live without.
        solved_at: None,
        fields: field_values,
        comments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn api_ticket(v: serde_json::Value) -> api::Ticket {
        serde_json::from_value(v).expect("ticket json")
    }

    #[test]
    fn people_map_role_email_activity_and_organisation() {
        let orgs = organizations(vec![api::Organization {
            id: 9,
            name: Some("Acme".into()),
        }]);
        let u: api::User = serde_json::from_value(json!({
            "id": 5, "name": "Ann", "email": "Ann@Example.COM", "role": "admin",
            "active": true, "suspended": true, "organization_id": 9
        }))
        .expect("user");
        let p = person(&u, &orgs);
        assert_eq!(p.source_id, "5");
        assert_eq!(p.email.as_deref(), Some("ann@example.com"));
        assert_eq!(p.role, PersonRole::Admin);
        assert!(!p.active, "suspended is inactive");
        assert_eq!(p.organization.as_deref(), Some("Acme"));

        let u: api::User =
            serde_json::from_value(json!({"id": 6, "role": "end-user", "email": null}))
                .expect("user");
        let p = person(&u, &orgs);
        assert_eq!(p.role, PersonRole::Requester);
        assert_eq!(p.email, None);
        assert_eq!(p.name, "Zendesk user 6");
        assert!(p.active);
    }

    #[test]
    fn ticket_maps_status_priority_watchers_and_fields() {
        let catalog = FieldCatalog::new(vec![
            serde_json::from_value::<api::TicketField>(json!({"id": 1, "title": "Region",
                "custom_field_options": [{"name": "Europe", "value": "eu"}]}))
            .expect("field"),
            serde_json::from_value::<api::TicketField>(json!({"id": 2, "title": "Products"}))
                .expect("field"),
        ]);
        let t = api_ticket(json!({
            "id": 42, "subject": "VPN down", "status": "hold", "priority": "urgent",
            "type": "incident", "tags": ["vpn"], "requester_id": 1, "assignee_id": 2,
            "collaborator_ids": [1, 3], "follower_ids": [3, 4], "email_cc_ids": [5],
            "created_at": "2024-01-01T00:00:00Z", "updated_at": "2024-01-02T00:00:00Z",
            "custom_fields": [
                {"id": 1, "value": "eu"}, {"id": 2, "value": ["a", "b"]},
                {"id": 3, "value": null}, {"id": 4, "value": false}, {"id": 5, "value": 7},
                {"id": 6, "value": ""}
            ]
        }));
        let at = ticket_created_at(&t).expect("created");
        let out = ticket(&t, at, &catalog, vec![]);
        assert_eq!(out.number, Some(42));
        assert_eq!(out.status, StatusKind::OnHold);
        assert_eq!(out.status_label, "hold");
        assert_eq!(out.priority, Some(PriorityLevel::Urgent));
        assert_eq!(out.ticket_type.as_deref(), Some("incident"));
        assert_eq!(out.watchers, vec!["3", "4", "5"]);
        assert_eq!(out.fields.get("Region").map(String::as_str), Some("Europe"));
        assert_eq!(out.fields.get("Products").map(String::as_str), Some("a, b"));
        assert_eq!(out.fields.get("Field 5").map(String::as_str), Some("7"));
        assert_eq!(out.fields.len(), 3);
    }

    #[test]
    fn comment_body_prefers_html_and_spots_zendesk_images() {
        let c: api::Comment = serde_json::from_value(json!({
            "id": 1, "html_body": "<p><img src=\"https://acme.zendesk.com/attachments/x\"></p>",
            "plain_body": "hi", "public": false
        }))
        .expect("comment");
        let b = body(&c);
        assert_eq!(b.format, BodyFormat::Html);
        assert!(has_zendesk_inline_image(&b));

        let c: api::Comment =
            serde_json::from_value(json!({"id": 2, "html_body": "", "body": "plain"}))
                .expect("comment");
        assert_eq!(
            body(&c),
            Body {
                format: BodyFormat::Plain,
                value: "plain".into()
            }
        );
    }
}
