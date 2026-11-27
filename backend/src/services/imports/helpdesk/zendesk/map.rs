//! Zendesk records to bundle records. Pure functions, no I/O: the API path
//! and the export path both come through here, so a ticket reads the same
//! whichever way it was fetched.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};

use super::api;
use crate::services::imports::helpdesk::bundle::{
    self, Attachment, Body, BodyFormat, Comment, Person, PersonRole, PriorityLevel, StatusKind,
    Ticket,
};

/// `s` without NUL characters, which Postgres text can't hold: one stray
/// NUL in a subject or body would fail its ticket's write on every run.
pub fn text(s: &str) -> String {
    if s.contains('\0') {
        s.replace('\0', "")
    } else {
        s.to_string()
    }
}

/// Organisation id → name, so a person carries their organisation by name.
/// A deleted organization is nobody's, so it isn't here.
pub type Organizations = HashMap<i64, String>;

pub fn organizations<'a>(orgs: impl IntoIterator<Item = &'a api::Organization>) -> Organizations {
    orgs.into_iter()
        .filter_map(organization)
        .filter(|o| !o.deleted)
        .filter_map(|o| Some((o.source_id.parse().ok()?, o.name)))
        .collect()
}

/// The bundle's record of an organization, or None when it has no name.
/// Whether its members shared tickets is `ticket_restriction` when Zendesk
/// sends it, else the older `shared_tickets`.
pub fn organization(o: &api::Organization) -> Option<bundle::Organization> {
    let name = text(o.name.as_deref()?.trim());
    if name.is_empty() {
        return None;
    }
    let shared_tickets = match o.ticket_restriction.as_deref() {
        Some(r) => r == "organization",
        None => o.shared_tickets.unwrap_or(false),
    };
    Some(bundle::Organization {
        source_id: o.id.to_string(),
        name,
        domain_names: o
            .domain_names
            .iter()
            .map(|d| text(d.trim()).to_lowercase())
            .filter(|d| !d.is_empty())
            .collect(),
        shared_tickets,
        deleted: o.deleted_at.is_some(),
    })
}

/// Staff with partial access at Zendesk: a light agent, a chat-only agent,
/// a contributor, an agent who may only write private comments, or another
/// account's agent through ticket sharing.
fn limited_staff(u: &api::User) -> bool {
    matches!(u.role_type, Some(1..=3)) || u.only_private_comments || u.shared_agent
}

pub fn person(u: &api::User, orgs: &Organizations) -> Person {
    let role = match u.role.as_deref() {
        Some("admin") => PersonRole::Admin,
        Some("agent") => PersonRole::Agent,
        // "end-user", and anything Zendesk adds later: the least access.
        _ => PersonRole::Requester,
    };
    // `active: false` is Zendesk's deleted; suspended is its own flag.
    let deleted = !u.active;
    let staff = role.is_staff();
    let phone = u
        .phone
        .as_deref()
        .map(|p| text(p.trim()))
        .filter(|p| !p.is_empty() && p.chars().count() <= 64);
    let email = u
        .email
        .as_deref()
        .map(|e| text(e.trim()).to_lowercase())
        .filter(|e| !e.is_empty());
    // Without a name, the address stands in, but never for someone deleted:
    // their history keeps a name, and nothing else of theirs.
    let name = u
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(text)
        .or_else(|| email.clone().filter(|_| u.active))
        .unwrap_or_else(|| format!("Zendesk user {}", u.id));
    Person {
        source_id: u.id.to_string(),
        email,
        name,
        role,
        active: u.active && !u.suspended,
        deleted,
        limited_staff: staff && limited_staff(u),
        custom_role: role == PersonRole::Agent && u.role_type == Some(0),
        organization: u.organization_id.and_then(|id| orgs.get(&id).cloned()),
        organization_id: u.organization_id.map(|id| id.to_string()),
        // Nothing of a deleted person's but their name, and never a shared
        // number, which is a switchboard rather than theirs.
        phone: phone.filter(|_| !deleted && !u.shared_phone_number),
        shared_phone: !deleted && u.shared_phone_number && u.phone.is_some(),
        created_at: u.created_at,
        updated_at: u.updated_at,
    }
}

/// Names for the ticket's group, organization and brand at Zendesk, for
/// its import note. A name the connector couldn't fetch is shown as the id.
#[derive(Debug, Default)]
pub struct Labels {
    pub organizations: Organizations,
    pub groups: HashMap<i64, String>,
    pub brands: HashMap<i64, String>,
}

impl Labels {
    pub fn names(items: impl IntoIterator<Item = api::Named>) -> HashMap<i64, String> {
        items
            .into_iter()
            .filter_map(|n| {
                let name = text(n.name?.trim());
                (!name.is_empty()).then_some((n.id, name))
            })
            .collect()
    }

    fn line(names: &HashMap<i64, String>, id: i64) -> String {
        names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("id {id}"))
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
                    .map(|t| text(t.trim()))
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| format!("Field {}", f.id));
                let options = f
                    .custom_field_options
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|o| Some((o.value?, text(&o.name?))))
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
        let rendered = match value {
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
        let rendered = text(rendered.trim());
        (!rendered.is_empty()).then_some(rendered)
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
            value: text(html),
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
        value: text(&plain),
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
/// `solved_at` is from the ticket's metrics, when Zendesk sent them.
pub fn ticket(
    t: &api::Ticket,
    created_at: DateTime<Utc>,
    fields: &FieldCatalog,
    labels: &Labels,
    mut comments: Vec<Comment>,
    solved_at: Option<DateTime<Utc>>,
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

    // Two fields can share a title; the second keeps its id beside it
    // rather than overwrite the first.
    let mut field_values: BTreeMap<String, String> = BTreeMap::new();
    for f in &t.custom_fields {
        let Some(value) = fields.render(f.id, &f.value) else {
            continue;
        };
        let title = fields.title(f.id);
        let key = if field_values.contains_key(&title) {
            format!("{title} (field {})", f.id)
        } else {
            title
        };
        field_values.insert(key, value);
    }
    for (label, id, names) in [
        ("Group there", t.group_id, &labels.groups),
        (
            "Organization there",
            t.organization_id,
            &labels.organizations,
        ),
        ("Brand there", t.brand_id, &labels.brands),
    ] {
        if let Some(id) = id {
            field_values
                .entry(label.to_string())
                .or_insert_with(|| Labels::line(names, id));
        }
    }

    let status_label = text(t.status.as_deref().unwrap_or("open"));
    let subject = t
        .subject
        .as_deref()
        .or(t.raw_subject.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(text)
        .unwrap_or_else(|| format!("Zendesk ticket #{}", t.id));

    Ticket {
        source_id: t.id.to_string(),
        number: Some(t.id),
        subject,
        status: status_kind(&status_label),
        status_label,
        priority: t.priority.as_deref().and_then(priority),
        ticket_type: t.ticket_type.as_deref().map(text).filter(|s| !s.is_empty()),
        tags: t.tags.iter().map(|tag| text(tag)).collect(),
        requester: requester.map(|id| id.to_string()),
        assignee: t.assignee_id.map(|id| id.to_string()),
        watchers,
        created_at,
        updated_at: t.updated_at.unwrap_or(created_at),
        solved_at: solved_at.or_else(|| t.metric_set.as_ref().and_then(|m| m.solved_at)),
        organization_id: t.organization_id.map(|id| id.to_string()),
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
        let acme: api::Organization =
            serde_json::from_value(json!({"id": 9, "name": "Acme"})).expect("org");
        let orgs = organizations([&acme]);
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
        assert!(!p.deleted, "suspended is not deleted");
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

    fn user(v: serde_json::Value) -> api::User {
        serde_json::from_value(v).expect("user")
    }

    #[test]
    fn limited_staff_from_role_type_and_private_comments() {
        let orgs = Organizations::new();
        for (v, limited, custom) in [
            (
                json!({"id": 1, "role": "agent", "role_type": 1}),
                true,
                false,
            ),
            (
                json!({"id": 2, "role": "agent", "role_type": 2}),
                true,
                false,
            ),
            (
                json!({"id": 3, "role": "agent", "role_type": 3}),
                true,
                false,
            ),
            (
                json!({"id": 4, "role": "agent", "only_private_comments": true}),
                true,
                false,
            ),
            (
                json!({"id": 5, "role": "agent", "shared_agent": true}),
                true,
                false,
            ),
            (
                json!({"id": 6, "role": "agent", "role_type": 0}),
                false,
                true,
            ),
            (
                json!({"id": 7, "role": "agent", "role_type": null}),
                false,
                false,
            ),
            (
                json!({"id": 8, "role": "admin", "role_type": 4}),
                false,
                false,
            ),
            // An end user is never staff, limited or not.
            (
                json!({"id": 9, "role": "end-user", "only_private_comments": true}),
                false,
                false,
            ),
        ] {
            let p = person(&user(v.clone()), &orgs);
            assert_eq!(p.limited_staff, limited, "{v}");
            assert_eq!(p.custom_role, custom, "{v}");
        }
    }

    #[test]
    fn deleted_is_not_suspended() {
        let orgs = Organizations::new();
        let deleted = person(
            &user(
                json!({"id": 1, "email": "d@x.test", "active": false, "phone": "+61 2 9999 0000"}),
            ),
            &orgs,
        );
        assert!(deleted.deleted && !deleted.active);
        assert_eq!(deleted.phone, None, "nothing but a deleted person's name");
        assert_eq!(
            deleted.name, "Zendesk user 1",
            "a deleted person without a name isn't named by their address"
        );
        let suspended = person(
            &user(json!({"id": 2, "email": "s@x.test", "active": true, "suspended": true})),
            &orgs,
        );
        assert!(!suspended.deleted && !suspended.active);
        let active = person(&user(json!({"id": 3, "active": null})), &orgs);
        assert!(!active.deleted && active.active);
    }

    #[test]
    fn shared_phone_is_dropped() {
        let orgs = Organizations::new();
        let own = person(&user(json!({"id": 1, "phone": " +61299990000 "})), &orgs);
        assert_eq!(own.phone.as_deref(), Some("+61299990000"));
        assert!(!own.shared_phone);
        let shared = person(
            &user(json!({"id": 2, "phone": "+61299990000", "shared_phone_number": true})),
            &orgs,
        );
        assert_eq!(shared.phone, None);
        assert!(shared.shared_phone);
    }

    #[test]
    fn organizations_read_sharing_and_a_deleted_one_is_nobodys() {
        let o = |v| serde_json::from_value::<api::Organization>(v).expect("org");
        let acme = o(
            json!({"id": 1, "name": "Acme", "domain_names": ["Acme.TEST"],
                            "shared_tickets": false, "ticket_restriction": "organization"}),
        );
        let beta = o(json!({"id": 2, "name": "Beta", "shared_tickets": true}));
        let gone = o(json!({"id": 3, "name": "Gone", "deleted_at": "2025-01-01T00:00:00Z"}));
        let rec = organization(&acme).expect("acme");
        assert!(rec.shared_tickets, "ticket_restriction wins");
        assert_eq!(rec.domain_names, vec!["acme.test"]);
        assert!(organization(&beta).expect("beta").shared_tickets);
        assert!(organization(&gone).expect("gone").deleted);
        let names = organizations([&acme, &beta, &gone]);
        assert_eq!(names.len(), 2);
        let u = user(json!({"id": 9, "organization_id": 3}));
        let p = person(&u, &names);
        assert_eq!(p.organization, None);
        assert_eq!(p.organization_id.as_deref(), Some("3"));
    }

    #[test]
    fn nul_characters_are_dropped_and_same_titled_fields_both_kept() {
        let catalog = FieldCatalog::new(vec![
            serde_json::from_value::<api::TicketField>(json!({"id": 1, "title": "Site"}))
                .expect("field"),
            serde_json::from_value::<api::TicketField>(json!({"id": 2, "title": "Site"}))
                .expect("field"),
        ]);
        let t = api_ticket(json!({
            "id": 7, "subject": "VPN\u{0} down", "tags": ["a\u{0}b"],
            "created_at": "2024-01-01T00:00:00Z",
            "custom_fields": [{"id": 1, "value": "north"}, {"id": 2, "value": "so\u{0}uth"}]
        }));
        let at = ticket_created_at(&t).expect("created");
        let out = ticket(&t, at, &catalog, &Labels::default(), vec![], None);
        assert_eq!(out.subject, "VPN down");
        assert_eq!(out.tags, vec!["ab"]);
        assert_eq!(out.fields.get("Site").map(String::as_str), Some("north"));
        assert_eq!(
            out.fields.get("Site (field 2)").map(String::as_str),
            Some("south")
        );
        let c: api::Comment = serde_json::from_value(json!({
            "id": 1, "html_body": "<p>a\u{0}b</p>", "public": null, "attachments": null
        }))
        .expect("comment with nulls");
        assert_eq!(body(&c).value, "<p>ab</p>");
        assert!(!c.public, "a comment that doesn't say is private");
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
        let out = ticket(&t, at, &catalog, &Labels::default(), vec![], None);
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
