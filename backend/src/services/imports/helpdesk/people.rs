//! Who each source person becomes in Nosdesk.
//!
//! Only people the bundle's tickets mention are brought across (requesters,
//! assignees, watchers, reply authors): a Zendesk account can hold tens of
//! thousands of end-users who never raised anything, and an import shouldn't
//! fill the workspace with them.
//!
//! The rules (design §2.7):
//! - A person already on the id map keeps their account.
//! - Matched by primary email to an active member: that member, whatever
//!   their role. The import never changes an existing account.
//! - Staff at the source:
//!   - named in the operator's agent map: the member it names, who must be
//!     staff here;
//!   - otherwise, on a self-hosted instance with agent creation on and the
//!     person active at the source: a new agent;
//!   - otherwise "former staff": a requester-level account, so their old
//!     replies keep their name without taking a seat. On Cloud staff seats
//!     belong to the control plane, so this is the default there.
//! - Requesters: their existing account, else a passwordless requester, the
//!   way inbound mail provisions one. No invitation is sent.
//! - No email at all: one placeholder author per source instance.

use std::collections::{HashMap, HashSet};

use diesel::QueryResult;
use uuid::Uuid;

use super::bundle::Person;
use crate::db::DbConnection;
use crate::models::{NewUser, WorkspaceRole};
use crate::repository::import_source_ids::{self as idmap, Entity, Scope};
use crate::repository::user_helpers::{self, CreateUserWithEmailOutcome, PrimaryEmailHolder};
use crate::repository::workspaces::SeatWriteAuthority;

/// How a person was resolved, for the plan and the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// Already brought across by an earlier run.
    Mapped,
    /// An active member with the same email.
    Member,
    /// Staff at the source, mapped by the operator to a staff member here.
    AgentMapped,
    /// Staff at the source, created as an agent here (self-hosted only).
    AgentCreated,
    /// Staff at the source with no seat here: a requester-level account.
    FormerStaff,
    /// A requester: their account, existing or new.
    Requester,
    /// No email: the source's placeholder author.
    Placeholder,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Mapped => "already imported",
            Outcome::Member => "existing member",
            Outcome::AgentMapped => "agent, mapped to a seat",
            Outcome::AgentCreated => "agent, created",
            Outcome::FormerStaff => "former staff (no seat)",
            Outcome::Requester => "requester",
            Outcome::Placeholder => "no email (placeholder author)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub uuid: Uuid,
    /// Holds a staff seat here, so can be an assignee and their public
    /// replies are staff replies.
    pub staff: bool,
    pub outcome: Outcome,
    /// Staff at the source. A former agent without a seat here is still the
    /// one who answered: their first public reply is the first response,
    /// and their replies are marked as staff replies on the comment.
    pub source_staff: bool,
    /// Their name at the source, kept on each reply they wrote.
    pub source_name: String,
}

impl Resolved {
    fn new(uuid: Uuid, staff: bool, outcome: Outcome) -> Self {
        Resolved {
            uuid,
            staff,
            outcome,
            source_staff: false,
            source_name: String::new(),
        }
    }
}

pub struct PeopleOptions<'a> {
    pub workspace_id: i32,
    pub scope: Scope<'a>,
    pub job_id: Uuid,
    /// Source email → Nosdesk email, lower-cased, for source staff.
    pub agent_map: &'a HashMap<String, String>,
    /// Create unmatched active source staff as agents. Refused on Cloud.
    pub create_agents: bool,
}

/// A planned decision, before anything is written.
#[derive(Debug, Clone)]
pub struct Decision {
    pub person: Person,
    pub outcome: Outcome,
    /// For staff, the Nosdesk email they map to, when they do.
    pub target_email: Option<String>,
}

/// The placeholder author's address: unique per source instance, never
/// deliverable (`.invalid` is reserved, RFC 2606).
pub fn placeholder_email(scope: Scope<'_>) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect()
    };
    format!(
        "unknown-author+{}-{}@import.invalid",
        clean(scope.source),
        clean(scope.instance)
    )
}

/// Decide, without writing, what happens to each of `people`.
pub fn plan(
    conn: &mut DbConnection,
    people: &[Person],
    opts: &PeopleOptions<'_>,
) -> QueryResult<Vec<Decision>> {
    let ids: Vec<String> = people.iter().map(|p| p.source_id.clone()).collect();
    let mapped = idmap::targets_of(conn, opts.scope, Entity::Person, &ids)?;
    let mut emails: Vec<String> = people.iter().filter_map(|p| p.email.clone()).collect();
    emails.extend(opts.agent_map.values().cloned());
    let holders = holders(conn, opts.workspace_id, &emails)?;

    Ok(people
        .iter()
        .map(|p| {
            let (outcome, target_email) = if mapped.contains_key(&p.source_id) {
                (Outcome::Mapped, None)
            } else {
                decide(p, &holders, opts)
            };
            Decision {
                person: p.clone(),
                outcome,
                target_email,
            }
        })
        .collect())
}

fn holders(
    conn: &mut DbConnection,
    workspace_id: i32,
    emails: &[String],
) -> QueryResult<HashMap<String, PrimaryEmailHolder>> {
    let mut out = HashMap::new();
    for chunk in emails.chunks(1000) {
        out.extend(user_helpers::primary_email_holders(
            conn,
            workspace_id,
            chunk,
        )?);
    }
    Ok(out)
}

fn is_staff_role(role: &str) -> bool {
    matches!(role, "owner" | "admin" | "agent")
}

fn decide(
    p: &Person,
    holders: &HashMap<String, PrimaryEmailHolder>,
    opts: &PeopleOptions<'_>,
) -> (Outcome, Option<String>) {
    let Some(email) = p.email.as_deref() else {
        return (Outcome::Placeholder, None);
    };
    if p.role.is_staff() {
        if let Some(target) = opts.agent_map.get(email) {
            if let Some(PrimaryEmailHolder::Member { role, .. }) = holders.get(target) {
                if is_staff_role(role) {
                    return (Outcome::AgentMapped, Some(target.clone()));
                }
            }
        }
    }
    if let Some(PrimaryEmailHolder::Member { .. }) = holders.get(email) {
        return (Outcome::Member, Some(email.to_string()));
    }
    if p.role.is_staff() {
        let can_create = opts.create_agents
            && p.active
            && !crate::repository::workspaces::staff_identity_externally_managed("agent")
            && !matches!(holders.get(email), Some(PrimaryEmailHolder::NotMember));
        return if can_create {
            (Outcome::AgentCreated, Some(email.to_string()))
        } else {
            (Outcome::FormerStaff, None)
        };
    }
    (Outcome::Requester, None)
}

/// Bring `decisions` across: create or find each account and record it on
/// the id map. Runs in the caller's transaction, under its import actor.
pub fn apply(
    conn: &mut DbConnection,
    decisions: &[Decision],
    opts: &PeopleOptions<'_>,
) -> QueryResult<HashMap<String, Resolved>> {
    let ids: Vec<String> = decisions
        .iter()
        .map(|d| d.person.source_id.clone())
        .collect();
    let mapped = idmap::targets_of(conn, opts.scope, Entity::Person, &ids)?;
    let mut emails: Vec<String> = decisions
        .iter()
        .filter_map(|d| d.person.email.clone())
        .collect();
    emails.extend(decisions.iter().filter_map(|d| d.target_email.clone()));
    let holders = holders(conn, opts.workspace_id, &emails)?;

    let mut out = HashMap::new();
    let mut placeholder: Option<Uuid> = None;
    for d in decisions {
        let p = &d.person;
        // A mapping whose account has since been purged is forgotten, and
        // the person brought across again.
        let live_target = match mapped.get(&p.source_id) {
            Some(target) => {
                let uuid = Uuid::parse_str(target)
                    .ok()
                    .filter(|u| user_exists(conn, *u));
                if uuid.is_none() {
                    idmap::forget(conn, opts.scope, Entity::Person, &p.source_id)?;
                }
                uuid
            }
            None => None,
        };
        let mut resolved = if let Some(uuid) = live_target {
            let staff = matches!(
                user_helpers::workspace_role(conn, uuid),
                Some(WorkspaceRole::Owner | WorkspaceRole::Admin | WorkspaceRole::Agent)
            );
            Resolved::new(uuid, staff, Outcome::Mapped)
        } else {
            let outcome = match d.outcome {
                Outcome::Mapped => decide(p, &holders, opts),
                other => (other, d.target_email.clone()),
            };
            let d = &Decision {
                person: p.clone(),
                outcome: outcome.0,
                target_email: outcome.1,
            };
            let resolved = match d.outcome {
                Outcome::Mapped => unreachable_mapped()?,
                Outcome::Member | Outcome::AgentMapped => {
                    let target = d.target_email.as_deref().unwrap_or_default();
                    match holders.get(target) {
                        Some(PrimaryEmailHolder::Member { user, role }) => {
                            Resolved::new(user.uuid, is_staff_role(role), d.outcome)
                        }
                        // The member left between plan and write: bring them
                        // across as the plan's fallback would.
                        _ => requester(conn, p, Outcome::Requester)?,
                    }
                }
                Outcome::AgentCreated => create_agent(conn, p)?,
                Outcome::FormerStaff => requester(conn, p, Outcome::FormerStaff)?,
                Outcome::Requester => requester(conn, p, Outcome::Requester)?,
                Outcome::Placeholder => {
                    let uuid = match placeholder {
                        Some(u) => u,
                        None => {
                            let name = format!("Unknown ({})", source_title(opts.scope.source));
                            let u = user_helpers::find_or_provision_requester(
                                &placeholder_email(opts.scope),
                                &name,
                                conn,
                                None,
                            )?
                            .uuid;
                            placeholder = Some(u);
                            u
                        }
                    };
                    Resolved::new(uuid, false, Outcome::Placeholder)
                }
            };
            idmap::record(
                conn,
                opts.scope,
                Entity::Person,
                &p.source_id,
                &resolved.uuid.to_string(),
                opts.job_id,
                p.updated_at.map(|t| t.naive_utc()),
            )?;
            resolved
        };
        resolved.source_staff = p.role.is_staff();
        resolved.source_name = display_name(p);
        out.insert(p.source_id.clone(), resolved);
    }
    Ok(out)
}

fn user_exists(conn: &mut DbConnection, uuid: Uuid) -> bool {
    use crate::schema::users;
    use diesel::prelude::*;
    users::table
        .find(uuid)
        .select(users::uuid)
        .first::<Uuid>(conn)
        .optional()
        .ok()
        .flatten()
        .is_some()
}

fn unreachable_mapped() -> QueryResult<Resolved> {
    // A plan said "mapped" but the map no longer has the row: someone
    // removed it mid-run. Fail the batch rather than guess.
    Err(diesel::result::Error::NotFound)
}

fn display_name(p: &Person) -> String {
    let name = p.name.trim();
    if !name.is_empty() {
        return name.chars().take(255).collect();
    }
    p.email
        .as_deref()
        .and_then(|e| e.split('@').next())
        .filter(|s| !s.is_empty())
        .unwrap_or("Unknown")
        .to_string()
}

fn requester(conn: &mut DbConnection, p: &Person, outcome: Outcome) -> QueryResult<Resolved> {
    let email = p.email.as_deref().ok_or(diesel::result::Error::NotFound)?;
    let user = user_helpers::find_or_provision_requester(email, &display_name(p), conn, None)?;
    let staff = matches!(
        user_helpers::workspace_role(conn, user.uuid),
        Some(WorkspaceRole::Owner | WorkspaceRole::Admin | WorkspaceRole::Agent)
    );
    Ok(Resolved::new(user.uuid, staff, outcome))
}

fn create_agent(conn: &mut DbConnection, p: &Person) -> QueryResult<Resolved> {
    let email = p.email.clone().ok_or(diesel::result::Error::NotFound)?;
    let new_user = NewUser {
        uuid: Uuid::now_v7(),
        name: display_name(p),
        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        microsoft_uuid: None,
        mfa_secret: None,
        mfa_secret_kek_id: None,
        mfa_enabled: false,
        platform_role: None,
    };
    match user_helpers::create_user_with_email(
        new_user,
        WorkspaceRole::Agent,
        email,
        false,
        Some("helpdesk_import".to_string()),
        conn,
        None,
        SeatWriteAuthority::Product,
    )? {
        CreateUserWithEmailOutcome::Created(user, _) => {
            Ok(Resolved::new(user.uuid, true, Outcome::AgentCreated))
        }
        // Hosted refused the seat after all: fall back to former staff.
        CreateUserWithEmailOutcome::ExternallyManaged => requester(conn, p, Outcome::FormerStaff),
    }
}

fn source_title(source: &str) -> String {
    match source {
        "zendesk" => "Zendesk".into(),
        "freshdesk" => "Freshdesk".into(),
        "freshservice" => "Freshservice".into(),
        other => other.to_string(),
    }
}

/// The source ids of everyone the bundle's tickets mention.
pub fn referenced(tickets: impl Iterator<Item = super::bundle::Ticket>) -> HashSet<String> {
    let mut out = HashSet::new();
    for t in tickets {
        out.extend(t.requester);
        out.extend(t.assignee);
        out.extend(t.watchers);
        for c in t.comments {
            out.extend(c.author);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::imports::helpdesk::bundle::PersonRole;

    fn person(email: Option<&str>, role: PersonRole, active: bool) -> Person {
        Person {
            source_id: "1".into(),
            email: email.map(str::to_string),
            name: "Pat".into(),
            role,
            active,
            organization: None,
            created_at: None,
            updated_at: None,
        }
    }

    fn opts(map: &HashMap<String, String>, create: bool) -> PeopleOptions<'_> {
        PeopleOptions {
            workspace_id: 1,
            scope: Scope {
                source: "zendesk",
                instance: "acme",
            },
            job_id: Uuid::nil(),
            agent_map: map,
            create_agents: create,
        }
    }

    #[test]
    fn no_email_is_a_placeholder_and_requesters_are_requesters() {
        let map = HashMap::new();
        let holders = HashMap::new();
        assert_eq!(
            decide(
                &person(None, PersonRole::Agent, true),
                &holders,
                &opts(&map, true)
            )
            .0,
            Outcome::Placeholder
        );
        assert_eq!(
            decide(
                &person(Some("r@x.com"), PersonRole::Requester, true),
                &holders,
                &opts(&map, true)
            )
            .0,
            Outcome::Requester
        );
    }

    #[test]
    fn unmatched_staff_are_former_staff_unless_creation_is_on_and_they_are_active() {
        let map = HashMap::new();
        let holders = HashMap::new();
        let agent = person(Some("a@x.com"), PersonRole::Agent, true);
        assert_eq!(
            decide(&agent, &holders, &opts(&map, false)).0,
            Outcome::FormerStaff
        );
        assert_eq!(
            decide(&agent, &holders, &opts(&map, true)).0,
            Outcome::AgentCreated
        );
        let gone = person(Some("a@x.com"), PersonRole::Agent, false);
        assert_eq!(
            decide(&gone, &holders, &opts(&map, true)).0,
            Outcome::FormerStaff
        );
    }

    #[test]
    fn placeholder_address_is_undeliverable_and_per_instance() {
        let a = placeholder_email(Scope {
            source: "zendesk",
            instance: "acme",
        });
        let b = placeholder_email(Scope {
            source: "zendesk",
            instance: "Other.Co",
        });
        assert!(a.ends_with("@import.invalid"));
        assert_ne!(a, b);
        assert_eq!(b, "unknown-author+zendesk-other-co@import.invalid");
    }
}
