//! Who each source person becomes in Nosdesk.
//!
//! Only people the bundle's tickets mention are brought across (requesters,
//! assignees, watchers, reply authors): a Zendesk account can hold tens of
//! thousands of end-users who never raised anything, and an import shouldn't
//! fill the workspace with them.
//!
//! Accounts are shared across workspaces, and a bundle is the source's word,
//! not proof of who owns an address here. So an address leads to an existing
//! account only on terms the workspace already has, and the import never
//! changes an existing account. The rules (design §2.7, tightened since):
//! - A person already on the id map keeps their account, while it is still
//!   one of the workspace's people.
//! - An address (an account's primary address, or a secondary one it
//!   verified, in any case) held by one of the workspace's people: that
//!   account. An unverified secondary address stands for nobody. An active member keeps their
//!   seat and role; a former member or someone the workspace's records name
//!   is reused without a seat being given back.
//! - Staff at the source named in the operator's agent map: the member it
//!   names, who must be staff here.
//! - An address that is the primary one of an account from elsewhere, for an
//!   active requester: that account, if the workspace may speak for it
//!   (`requester_identities::out_of_reach`), seated here as a requester the
//!   way inbound mail seats one. Otherwise, and for any other address an
//!   account elsewhere holds, nobody's account is touched: the person's
//!   replies and requests are credited to the placeholder author, with their
//!   name and address kept on each.
//! - An address no account holds: a new account. An active requester gets a
//!   requester seat; active source staff become agents on a self-hosted
//!   instance with agent creation on. Anyone else (former staff, people
//!   suspended or deleted at the source) gets an account without a seat, so
//!   their history keeps their name and nothing signs them in: they are the
//!   workspace's people by a profile kept for them there, as a directory
//!   contact is.
//! - No email at all: the placeholder author, one per source instance and
//!   workspace, which no one can sign in as.
//!
//! No invitation is sent to anyone.

use std::collections::{HashMap, HashSet};

use diesel::{Connection, QueryResult};
use uuid::Uuid;

use super::bundle::Person;
use crate::db::DbConnection;
use crate::models::{NewUser, WorkspaceRole};
use crate::repository::import_source_ids::{self as idmap, Entity, Scope, PLACEHOLDER_PERSON};
use crate::repository::user_helpers::{self, AddressHolder, CreateUserWithEmailOutcome};
use crate::repository::workspaces::{self, SeatWriteAuthority};

/// How a person was resolved, for the plan and the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// Already brought across by an earlier run.
    Mapped,
    /// An active member with the same address.
    Member,
    /// One of the workspace's people without an active seat: a former
    /// member, or someone its records name. Reused; no seat is given back.
    Known,
    /// Staff at the source, mapped by the operator to a staff member here.
    AgentMapped,
    /// Staff at the source, created as an agent here (self-hosted only).
    AgentCreated,
    /// A requester with no account anywhere: created, with a requester seat.
    Requester,
    /// A requester whose account began in another workspace: seated here as
    /// a requester.
    Reused,
    /// Staff at the source with no seat here: an account without a seat.
    FormerStaff,
    /// Suspended or deleted at the source: an account without a seat.
    Inactive,
    /// Their address belongs to an account beyond this workspace's reach:
    /// credited to the placeholder, with their name and address kept.
    Withheld,
    /// No email: the source's placeholder author.
    Placeholder,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Mapped => "already imported",
            Outcome::Member => "existing member",
            Outcome::Known => "known here, no seat",
            Outcome::AgentMapped => "agent, mapped to a seat",
            Outcome::AgentCreated => "agent, created",
            Outcome::Requester => "requester, created",
            Outcome::Reused => "requester, account from elsewhere",
            Outcome::FormerStaff => "former staff, no seat",
            Outcome::Inactive => "suspended at the source, no seat",
            Outcome::Withheld => "address held elsewhere (placeholder)",
            Outcome::Placeholder => "no email (placeholder author)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub uuid: Uuid,
    /// Can work tickets here (an active staff seat), so can be an assignee
    /// and their public replies are staff replies.
    pub staff: bool,
    /// Holds an active seat here, so can watch a ticket and hear of it.
    pub seated: bool,
    pub outcome: Outcome,
    /// Staff at the source. A former agent without a seat here is still the
    /// one who answered: their first public reply is the first response,
    /// and their replies are marked as staff replies on the comment.
    pub source_staff: bool,
    /// Their name at the source, kept on each reply they wrote.
    pub source_name: String,
    /// Their address at the source, kept on each reply when the reply is
    /// credited to the placeholder in their stead.
    pub source_email: Option<String>,
}

impl Resolved {
    fn new(uuid: Uuid, staff: bool, seated: bool, outcome: Outcome) -> Self {
        Resolved {
            uuid,
            staff,
            seated,
            outcome,
            source_staff: false,
            source_name: String::new(),
            source_email: None,
        }
    }

    /// Credited to the placeholder in their stead.
    pub fn withheld(&self) -> bool {
        self.outcome == Outcome::Withheld
    }
}

pub struct PeopleOptions<'a> {
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

/// What [`plan`] found.
#[derive(Debug, Clone, Default)]
pub struct PeoplePlan {
    pub decisions: Vec<Decision>,
    /// Agent map entries that map nobody, in words for the operator.
    pub warnings: Vec<String>,
}

/// Where the workspace stands on the bundle's people, read once per pass.
struct Facts {
    /// Source id → mapped account, when the account is still one of the
    /// workspace's people.
    mapped: HashMap<String, Uuid>,
    /// Address (lower-case) → the account holding it.
    holders: HashMap<String, AddressHolder>,
    /// Accounts from elsewhere the workspace may speak for.
    within_reach: HashSet<Uuid>,
}

fn facts(
    conn: &mut DbConnection,
    people: &[Person],
    opts: &PeopleOptions<'_>,
) -> QueryResult<Facts> {
    let ws = opts.scope.workspace_id;
    let ids: Vec<String> = people.iter().map(|p| p.source_id.clone()).collect();
    let targets = idmap::targets_of(conn, opts.scope, Entity::Person, &ids)?;
    let target_uuids: Vec<Uuid> = targets
        .values()
        .filter_map(|t| Uuid::parse_str(t).ok())
        .collect();
    let live = crate::repository::directory::people_among(conn, ws, &target_uuids)?;
    let mapped = targets
        .into_iter()
        .filter_map(|(id, t)| Some((id, Uuid::parse_str(&t).ok()?)))
        .filter(|(_, u)| live.contains(u))
        .collect();

    let mut emails: Vec<String> = people.iter().filter_map(|p| p.email.clone()).collect();
    emails.extend(opts.agent_map.values().cloned());
    let mut holders = HashMap::new();
    for chunk in emails.chunks(1000) {
        holders.extend(user_helpers::address_holders(conn, ws, chunk)?);
    }

    let mut within_reach = HashSet::new();
    let mut checked = HashSet::new();
    for h in holders.values() {
        if h.person || !h.primary || !h.trusted || !checked.insert(h.uuid) {
            continue;
        }
        if crate::repository::requester_identities::out_of_reach(conn, ws, h.uuid)?.is_none() {
            within_reach.insert(h.uuid);
        }
    }
    Ok(Facts {
        mapped,
        holders,
        within_reach,
    })
}

/// The placeholder address: undeliverable (`.invalid` is reserved, RFC
/// 2606) and unguessable, so no account elsewhere can hold it first.
fn placeholder_email() -> String {
    format!("unknown-author+{}@import.invalid", Uuid::now_v7().simple())
}

/// The source's placeholder author in this workspace: found on the id map,
/// or created without a seat, so no one signs in as it. Run under the
/// source's lock (`idmap::lock_source`), so two runs make one.
pub fn placeholder(conn: &mut DbConnection, opts: &PeopleOptions<'_>) -> QueryResult<Uuid> {
    let scope = opts.scope;
    if let Some(target) = idmap::target_of(conn, scope, Entity::Person, PLACEHOLDER_PERSON)? {
        if let Ok(uuid) = Uuid::parse_str(&target) {
            if crate::repository::directory::is_person(conn, scope.workspace_id, uuid)? {
                return Ok(uuid);
            }
        }
        idmap::forget(conn, scope, Entity::Person, PLACEHOLDER_PERSON)?;
    }
    let name = format!("Unknown ({})", source_title(scope.source));
    let uuid = create_account(conn, &name, &placeholder_email(), Seat::None)?;
    idmap::record(
        conn,
        scope,
        Entity::Person,
        PLACEHOLDER_PERSON,
        &uuid.to_string(),
        opts.job_id,
        None,
    )?;
    Ok(uuid)
}

/// Decide, without writing, what happens to each of `people`.
pub fn plan(
    conn: &mut DbConnection,
    people: &[Person],
    opts: &PeopleOptions<'_>,
) -> QueryResult<PeoplePlan> {
    let facts = facts(conn, people, opts)?;
    let decisions = people
        .iter()
        .map(|p| {
            let (outcome, target_email) = if facts.mapped.contains_key(&p.source_id) {
                (Outcome::Mapped, None)
            } else {
                decide(p, &facts, opts)
            };
            Decision {
                person: p.clone(),
                outcome,
                target_email,
            }
        })
        .collect();

    let mut warnings = Vec::new();
    let mut entries: Vec<(&String, &String)> = opts.agent_map.iter().collect();
    entries.sort();
    for (from, to) in entries {
        let named = people.iter().any(|p| {
            p.role.is_staff()
                && p.email.as_deref().map(str::to_lowercase).as_deref() == Some(from.as_str())
        });
        if !named {
            warnings.push(format!(
                "--map-agent {from}: no one the tickets mention was staff at the source with that address"
            ));
        } else if !is_staff_holder(facts.holders.get(&to.to_lowercase())) {
            warnings.push(format!(
                "--map-agent {from}={to}: {to} is not staff in this workspace, so {from} isn't mapped to them"
            ));
        }
    }
    Ok(PeoplePlan {
        decisions,
        warnings,
    })
}

fn is_staff_role(role: &str) -> bool {
    matches!(role, "owner" | "admin" | "agent")
}

fn is_staff_holder(h: Option<&AddressHolder>) -> bool {
    h.filter(|h| h.trusted)
        .and_then(|h| h.active_role.as_deref())
        .is_some_and(is_staff_role)
}

fn decide(p: &Person, facts: &Facts, opts: &PeopleOptions<'_>) -> (Outcome, Option<String>) {
    let Some(email) = p.email.as_deref().map(str::to_lowercase) else {
        return (Outcome::Placeholder, None);
    };
    let email = email.as_str();
    let staff = p.role.is_staff();
    if staff {
        if let Some(target) = opts.agent_map.get(email) {
            if is_staff_holder(facts.holders.get(&target.to_lowercase())) {
                return (Outcome::AgentMapped, Some(target.clone()));
            }
        }
    }
    match facts.holders.get(email) {
        // An unverified secondary address names nobody, and no new account
        // can take it either.
        Some(h) if !h.trusted => (Outcome::Withheld, None),
        Some(h) if h.active_role.is_some() => (Outcome::Member, Some(email.to_string())),
        Some(h) if h.person => (Outcome::Known, Some(email.to_string())),
        Some(h) if h.primary && !staff && p.active && facts.within_reach.contains(&h.uuid) => {
            (Outcome::Reused, Some(email.to_string()))
        }
        Some(_) => (Outcome::Withheld, None),
        None if staff
            && opts.create_agents
            && p.active
            && !workspaces::staff_identity_externally_managed("agent") =>
        {
            (Outcome::AgentCreated, Some(email.to_string()))
        }
        None if staff => (Outcome::FormerStaff, None),
        None if !p.active => (Outcome::Inactive, None),
        None => (Outcome::Requester, None),
    }
}

/// What [`apply`] did.
#[derive(Debug, Default)]
pub struct Applied {
    pub resolved: HashMap<String, Resolved>,
    /// People who couldn't be brought across as decided, what happened
    /// instead, and why.
    pub problems: Vec<String>,
}

/// Bring `people` across: find or create each account and record it on the
/// id map. Decides again from the workspace as it is now, so a plan that
/// went stale can't attach anyone it shouldn't. Runs in the caller's
/// transaction, under its import actor and the source's lock; `placeholder`
/// is [`placeholder`]'s answer. Each person is written in a savepoint: one
/// that fails (a full seat limit, say) falls back to an account without a
/// seat, or to the placeholder, and is reported, and the rest carry on.
pub fn apply(
    conn: &mut DbConnection,
    people: &[Person],
    opts: &PeopleOptions<'_>,
    placeholder: Uuid,
) -> QueryResult<Applied> {
    let ws = opts.scope.workspace_id;
    let facts = facts(conn, people, opts)?;
    // A mapping whose account is no longer one of the workspace's people
    // (purged, or never was) is forgotten, and the person brought across
    // again.
    let ids: Vec<String> = people.iter().map(|p| p.source_id.clone()).collect();
    for id in idmap::targets_of(conn, opts.scope, Entity::Person, &ids)?.keys() {
        if !facts.mapped.contains_key(id) {
            idmap::forget(conn, opts.scope, Entity::Person, id)?;
        }
    }

    let mut out = Applied::default();
    for p in people {
        let mut resolved = match facts.mapped.get(&p.source_id) {
            Some(uuid) => standing(conn, ws, *uuid, Outcome::Mapped)?,
            None => {
                let (outcome, target) = decide(p, &facts, opts);
                let first = conn.transaction(|conn| {
                    let r = bring(
                        conn,
                        p,
                        outcome,
                        target.as_deref(),
                        &facts,
                        opts,
                        placeholder,
                    )?;
                    remember(conn, p, &r, opts)?;
                    Ok::<_, diesel::result::Error>(r)
                });
                match first {
                    Ok(r) => r,
                    Err(e) => {
                        let fallback = conn
                            .transaction(|conn| {
                                // Staff keep their name on an account without a
                                // seat; anyone else goes to the placeholder.
                                if !p.role.is_staff() || p.email.is_none() {
                                    return Err(diesel::result::Error::RollbackTransaction);
                                }
                                let uuid = create_account(
                                    conn,
                                    &display_name(p),
                                    &new_address(p)?,
                                    Seat::None,
                                )?;
                                let r = Resolved::new(uuid, false, false, Outcome::FormerStaff);
                                remember(conn, p, &r, opts)?;
                                Ok(r)
                            })
                            .unwrap_or_else(|_| {
                                Resolved::new(placeholder, false, false, Outcome::Withheld)
                            });
                        out.problems.push(format!(
                            "{}: {} ({e}), so {}",
                            display_name(p),
                            outcome.label(),
                            if fallback.withheld() {
                                "credited to the placeholder author"
                            } else {
                                "brought across without a seat"
                            }
                        ));
                        fallback
                    }
                }
            }
        };
        resolved.source_staff = p.role.is_staff();
        resolved.source_name = display_name(p);
        resolved.source_email = p.email.clone();
        out.resolved.insert(p.source_id.clone(), resolved);
    }
    Ok(out)
}

/// Record `r` on the id map, unless it stands for the placeholder.
fn remember(
    conn: &mut DbConnection,
    p: &Person,
    r: &Resolved,
    opts: &PeopleOptions<'_>,
) -> QueryResult<()> {
    if matches!(r.outcome, Outcome::Withheld | Outcome::Placeholder) {
        return Ok(());
    }
    idmap::record(
        conn,
        opts.scope,
        Entity::Person,
        &p.source_id,
        &r.uuid.to_string(),
        opts.job_id,
        p.updated_at.map(|t| t.naive_utc()),
    )
}

/// An existing account as it stands in the workspace.
fn standing(
    conn: &mut DbConnection,
    workspace_id: i32,
    uuid: Uuid,
    outcome: Outcome,
) -> QueryResult<Resolved> {
    let role = workspaces::membership(conn, workspace_id, uuid)?.map(|m| m.role);
    Ok(Resolved::new(
        uuid,
        role.as_deref().is_some_and(is_staff_role),
        role.is_some(),
        outcome,
    ))
}

fn bring(
    conn: &mut DbConnection,
    p: &Person,
    outcome: Outcome,
    target: Option<&str>,
    facts: &Facts,
    opts: &PeopleOptions<'_>,
    placeholder: Uuid,
) -> QueryResult<Resolved> {
    let ws = opts.scope.workspace_id;
    let withheld = || Resolved::new(placeholder, false, false, Outcome::Withheld);
    let holder = target.and_then(|t| facts.holders.get(&t.to_lowercase()));
    match (outcome, holder) {
        (Outcome::Member | Outcome::AgentMapped, Some(h)) => standing(conn, ws, h.uuid, outcome),
        (Outcome::Known, Some(h)) => {
            // Someone the workspace knows, without a seat: an active
            // requester joins as one (a seat the team removed stays
            // removed), anyone else stays as they are.
            if p.active && !p.role.is_staff() {
                workspaces::join_as_requester(conn, ws, h.uuid)?;
            }
            standing(conn, ws, h.uuid, outcome)
        }
        (Outcome::Reused, Some(h)) => {
            workspaces::join_as_requester(conn, ws, h.uuid)?;
            // The join refuses an account it can't vouch for (no seat
            // anywhere, deleted); that one is not attached either.
            if crate::repository::directory::is_person(conn, ws, h.uuid)? {
                standing(conn, ws, h.uuid, outcome)
            } else {
                Ok(withheld())
            }
        }
        (Outcome::AgentCreated, _) => create_agent(conn, p),
        (Outcome::Requester, _) => {
            let uuid = create_account(conn, &display_name(p), &new_address(p)?, Seat::Requester)?;
            Ok(Resolved::new(uuid, false, true, outcome))
        }
        (Outcome::FormerStaff | Outcome::Inactive, _) => {
            let uuid = create_account(conn, &display_name(p), &new_address(p)?, Seat::None)?;
            Ok(Resolved::new(uuid, false, false, outcome))
        }
        (Outcome::Placeholder, _) => Ok(Resolved::new(placeholder, false, false, outcome)),
        // Withheld, or a holder that went away between the read and now.
        _ => Ok(withheld()),
    }
}

fn email_of(p: &Person) -> QueryResult<&str> {
    p.email.as_deref().ok_or(diesel::result::Error::NotFound)
}

/// A new account's address: lower-cased, as every address is compared.
fn new_address(p: &Person) -> QueryResult<String> {
    Ok(email_of(p)?.to_lowercase())
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

/// The seat a new account gets here.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seat {
    Requester,
    /// None: one of the workspace's people by a profile kept for them
    /// there, with no membership, so nothing signs them in and a seat
    /// granted later is an ordinary new one.
    None,
}

fn new_user(name: &str) -> NewUser {
    NewUser {
        uuid: Uuid::now_v7(),
        name: name.to_string(),
        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        microsoft_uuid: None,
        mfa_secret: None,
        mfa_secret_kek_id: None,
        mfa_enabled: false,
        platform_role: None,
    }
}

/// A new passwordless account for `email`, which no account holds.
fn create_account(
    conn: &mut DbConnection,
    name: &str,
    email: &str,
    seat: Seat,
) -> QueryResult<Uuid> {
    if seat == Seat::None {
        return Ok(user_helpers::create_contact_with_email(
            conn,
            new_user(name),
            email.to_string(),
            Some("helpdesk_import".to_string()),
        )?
        .uuid);
    }
    let (user, _) = user_helpers::create_user_with_email(
        new_user(name),
        WorkspaceRole::Member,
        email.to_string(),
        false,
        Some("helpdesk_import".to_string()),
        conn,
        None,
        SeatWriteAuthority::Product,
    )?
    .into_created()?;
    Ok(user.uuid)
}

fn create_agent(conn: &mut DbConnection, p: &Person) -> QueryResult<Resolved> {
    let email = new_address(p)?;
    match user_helpers::create_user_with_email(
        new_user(&display_name(p)),
        WorkspaceRole::Agent,
        email.clone(),
        false,
        Some("helpdesk_import".to_string()),
        conn,
        None,
        SeatWriteAuthority::Product,
    )? {
        CreateUserWithEmailOutcome::Created(user, _) => {
            Ok(Resolved::new(user.uuid, true, true, Outcome::AgentCreated))
        }
        // Hosted refused the seat after all: an account without one.
        CreateUserWithEmailOutcome::ExternallyManaged => {
            let uuid = create_account(conn, &display_name(p), &email, Seat::None)?;
            Ok(Resolved::new(uuid, false, false, Outcome::FormerStaff))
        }
    }
}

pub fn source_title(source: &str) -> String {
    match source {
        "zendesk" => "Zendesk".into(),
        "freshdesk" => "Freshdesk".into(),
        "freshservice" => "Freshservice".into(),
        other => other.to_string(),
    }
}

/// The source ids of everyone `t` mentions.
pub fn mentioned(t: &super::bundle::Ticket, out: &mut HashSet<String>) {
    out.extend(t.requester.iter().cloned());
    out.extend(t.assignee.iter().cloned());
    out.extend(t.watchers.iter().cloned());
    for c in &t.comments {
        out.extend(c.author.iter().cloned());
    }
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
            scope: Scope {
                workspace_id: 1,
                source: "zendesk",
                instance: "acme",
            },
            job_id: Uuid::nil(),
            agent_map: map,
            create_agents: create,
        }
    }

    fn facts(holders: Vec<(&str, AddressHolder)>, reach: &[Uuid]) -> Facts {
        Facts {
            mapped: HashMap::new(),
            holders: holders
                .into_iter()
                .map(|(e, h)| (e.to_string(), h))
                .collect(),
            within_reach: reach.iter().copied().collect(),
        }
    }

    fn holder(uuid: Uuid, primary: bool, role: Option<&str>, person: bool) -> AddressHolder {
        AddressHolder {
            uuid,
            primary,
            trusted: true,
            active_role: role.map(str::to_string),
            person,
        }
    }

    #[test]
    fn no_email_is_a_placeholder_and_new_requesters_are_created() {
        let map = HashMap::new();
        let f = facts(vec![], &[]);
        let o = opts(&map, true);
        assert_eq!(
            decide(&person(None, PersonRole::Agent, true), &f, &o).0,
            Outcome::Placeholder
        );
        let active = person(Some("r@x.com"), PersonRole::Requester, true);
        assert_eq!(decide(&active, &f, &o).0, Outcome::Requester);
        let suspended = person(Some("r@x.com"), PersonRole::Requester, false);
        assert_eq!(decide(&suspended, &f, &o).0, Outcome::Inactive);
    }

    #[test]
    fn unmatched_staff_are_former_staff_unless_creation_is_on_and_they_are_active() {
        let map = HashMap::new();
        let f = facts(vec![], &[]);
        let agent = person(Some("a@x.com"), PersonRole::Agent, true);
        assert_eq!(
            decide(&agent, &f, &opts(&map, false)).0,
            Outcome::FormerStaff
        );
        assert_eq!(
            decide(&agent, &f, &opts(&map, true)).0,
            Outcome::AgentCreated
        );
        let gone = person(Some("a@x.com"), PersonRole::Agent, false);
        assert_eq!(decide(&gone, &f, &opts(&map, true)).0, Outcome::FormerStaff);
    }

    #[test]
    fn accounts_from_elsewhere_are_reused_only_within_reach_by_primary_address() {
        let map = HashMap::new();
        let o = opts(&map, true);
        let u = Uuid::now_v7();
        let r = person(Some("r@x.com"), PersonRole::Requester, true);
        let reach = facts(vec![("r@x.com", holder(u, true, None, false))], &[u]);
        assert_eq!(decide(&r, &reach, &o).0, Outcome::Reused);
        let beyond = facts(vec![("r@x.com", holder(u, true, None, false))], &[]);
        assert_eq!(decide(&r, &beyond, &o).0, Outcome::Withheld);
        let secondary = facts(vec![("r@x.com", holder(u, false, None, false))], &[u]);
        assert_eq!(decide(&r, &secondary, &o).0, Outcome::Withheld);
        // Staff at the source never take an account from elsewhere.
        let agent = person(Some("r@x.com"), PersonRole::Agent, true);
        assert_eq!(decide(&agent, &reach, &o).0, Outcome::Withheld);
        // A secondary address of one of the workspace's own people matches.
        let ours = facts(
            vec![("r@x.com", holder(u, false, Some("member"), true))],
            &[],
        );
        assert_eq!(decide(&r, &ours, &o).0, Outcome::Member);
        let former = facts(vec![("r@x.com", holder(u, true, None, true))], &[]);
        assert_eq!(decide(&agent, &former, &o).0, Outcome::Known);
    }

    #[test]
    fn the_agent_map_needs_staff_here() {
        let map = HashMap::from([("a@x.com".to_string(), "b@here.com".to_string())]);
        let o = opts(&map, false);
        let agent = person(Some("a@x.com"), PersonRole::Agent, true);
        let u = Uuid::now_v7();
        let staff = facts(
            vec![("b@here.com", holder(u, true, Some("agent"), true))],
            &[],
        );
        assert_eq!(decide(&agent, &staff, &o).0, Outcome::AgentMapped);
        let requester = facts(
            vec![("b@here.com", holder(u, true, Some("member"), true))],
            &[],
        );
        assert_eq!(decide(&agent, &requester, &o).0, Outcome::FormerStaff);
    }

    #[test]
    fn placeholder_addresses_are_undeliverable_and_unguessable() {
        let a = placeholder_email();
        assert!(a.ends_with("@import.invalid"));
        assert_ne!(a, placeholder_email());
    }
}
