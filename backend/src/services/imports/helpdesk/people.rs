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
//! - Deleted at the source (often an erasure request): their history only.
//!   One of the workspace's people with their address keeps it; otherwise
//!   the placeholder author, with their name on each reply and no address
//!   stored or account created.
//! - Limited staff at the source (light agents, chat-only agents,
//!   contributors, another account's agents) are never created as agents;
//!   the operator's agent map is the only way they get a seat.
//!
//! People who share one address at the source (Zendesk lets several end
//! users use one) are decided once, together, whatever the batch: the
//! least privileged role among them, a seat when any is active, and the
//! earliest-created active member's name. Each keeps their own name on the
//! replies they wrote.
//!
//! Once someone is brought across, their profile here gets the
//! organization and phone the source had for them, once, when empty: a
//! value set or cleared here is left as it is, and a profile a directory
//! sync owns is left alone.
//!
//! No invitation is sent to anyone.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDateTime;
use diesel::{Connection, QueryResult};
use uuid::Uuid;

use super::bundle::{Person, PersonRole};
use crate::db::DbConnection;
use crate::models::{NewUser, WorkspaceRole};
use crate::repository::import_source_ids::{self as idmap, Entity, Scope, PLACEHOLDER_PERSON};
use crate::repository::user_contact::{self, OrganizationFill};
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
    /// Deleted at the source and no one here: credited to the placeholder,
    /// with their name kept and nothing else.
    Deleted,
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
            Outcome::Deleted => "deleted at the source (placeholder)",
        }
    }

    /// Their history is credited to the placeholder author.
    pub fn on_placeholder(self) -> bool {
        matches!(
            self,
            Outcome::Withheld | Outcome::Placeholder | Outcome::Deleted
        )
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

    /// Deleted at the source and credited to the placeholder.
    pub fn deleted(&self) -> bool {
        self.outcome == Outcome::Deleted
    }
}

/// One address and everyone at the source who used it, decided together.
/// Someone with no address is a group of one.
#[derive(Debug, Clone)]
pub struct AddressGroup {
    /// Who the group is decided as: see [`representative`].
    pub rep: Person,
    /// Everyone in it, in the bundle's order.
    pub members: Vec<Person>,
}

impl AddressGroup {
    pub fn shared(&self) -> bool {
        self.members.len() > 1
    }
}

/// The bundle's people grouped by lower-cased address, each group in the
/// order its first member came. Done before anything is decided or
/// batched, so a shared address resolves the same in any batch and on any
/// run.
pub fn group_by_address(people: Vec<Person>) -> Vec<AddressGroup> {
    let mut groups: Vec<Vec<Person>> = Vec::new();
    let mut by_email: HashMap<String, usize> = HashMap::new();
    for p in people {
        match p.email.as_deref().map(str::to_lowercase) {
            Some(e) => match by_email.get(&e) {
                Some(&i) => groups[i].push(p),
                None => {
                    by_email.insert(e, groups.len());
                    groups.push(vec![p]);
                }
            },
            None => groups.push(vec![p]),
        }
    }
    groups
        .into_iter()
        .map(|members| AddressGroup {
            rep: representative(&members),
            members,
        })
        .collect()
}

/// Earliest created first; no date last; then by id, so the choice never
/// depends on order.
fn created_key(p: &Person) -> (bool, Option<NaiveDateTime>, Option<i64>, String) {
    (
        p.created_at.is_none(),
        p.created_at.map(|t| t.naive_utc()),
        p.source_id.parse().ok(),
        p.source_id.clone(),
    )
}

fn privilege(role: PersonRole) -> u8 {
    match role {
        PersonRole::Requester => 0,
        PersonRole::Agent => 1,
        PersonRole::Admin => 2,
    }
}

/// Who a group of people sharing an address is decided as. Deleted members
/// count for attribution only, unless everyone is deleted. Of the rest:
/// the least privileged role (anyone who raised tickets makes it a
/// requester), active when any is, limited staff when any is, and the name,
/// organization and phone of the earliest-created active member.
fn representative(members: &[Person]) -> Person {
    let alive: Vec<&Person> = members.iter().filter(|p| !p.deleted).collect();
    let pool: Vec<&Person> = if alive.is_empty() {
        members.iter().collect()
    } else {
        alive
    };
    let chosen = pool
        .iter()
        .filter(|p| p.active)
        .min_by_key(|p| created_key(p))
        .or_else(|| pool.iter().min_by_key(|p| created_key(p)))
        .copied()
        .cloned()
        .unwrap_or_else(|| members[0].clone());
    let role = pool
        .iter()
        .map(|p| p.role)
        .min_by_key(|r| privilege(*r))
        .unwrap_or(chosen.role);
    Person {
        role,
        active: pool.iter().any(|p| p.active && !p.deleted),
        deleted: pool.iter().all(|p| p.deleted),
        limited_staff: role.is_staff() && pool.iter().any(|p| p.limited_staff),
        custom_role: role.is_staff() && pool.iter().any(|p| p.custom_role),
        ..chosen
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
    /// One for each person, members of a shared address included.
    pub decisions: Vec<Decision>,
    /// Agent map entries that map nobody, in words for the operator.
    pub warnings: Vec<String>,
    pub profile: ProfileTally,
    /// People brought across before whom the source has changed since.
    pub changed_at_source: Vec<String>,
}

/// Where the workspace stands on the bundle's people, read once per pass.
struct Facts {
    /// Source id → mapped account, when the account is still one of the
    /// workspace's people.
    mapped: HashMap<String, Uuid>,
    /// When the source last changed each mapped person, as of the import
    /// that mapped them.
    mapped_updated: HashMap<String, Option<NaiveDateTime>>,
    /// People whose profile fill was done by an earlier run.
    profiled: HashSet<String>,
    /// Address (lower-case) → the account holding it.
    holders: HashMap<String, AddressHolder>,
    /// Accounts from elsewhere the workspace may speak for.
    within_reach: HashSet<Uuid>,
}

fn facts(
    conn: &mut DbConnection,
    groups: &[AddressGroup],
    opts: &PeopleOptions<'_>,
) -> QueryResult<Facts> {
    let ws = opts.scope.workspace_id;
    let people: Vec<&Person> = groups.iter().flat_map(|g| &g.members).collect();
    let ids: Vec<String> = people.iter().map(|p| p.source_id.clone()).collect();
    let targets = idmap::targets_of(conn, opts.scope, Entity::Person, &ids)?;
    let target_uuids: Vec<Uuid> = targets
        .values()
        .filter_map(|t| Uuid::parse_str(t).ok())
        .collect();
    let live = crate::repository::directory::people_among(conn, ws, &target_uuids)?;
    let mapped: HashMap<String, Uuid> = targets
        .into_iter()
        .filter_map(|(id, t)| Some((id, Uuid::parse_str(&t).ok()?)))
        .filter(|(_, u)| live.contains(u))
        .collect();
    let mapped_ids: Vec<String> = mapped.keys().cloned().collect();
    let mapped_updated =
        idmap::source_updated_at_of(conn, opts.scope, Entity::Person, &mapped_ids)?;
    let profiled = idmap::targets_of(conn, opts.scope, Entity::Profile, &ids)?
        .into_keys()
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
        mapped_updated,
        profiled,
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

/// What the profile fill would do or did: organizations and phones, by
/// account.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileTally {
    /// Organizations written (or, in a plan, to write).
    pub organizations: u64,
    /// Organizations left as they were: one is set here already.
    pub organizations_kept: u64,
    /// Profiles a directory sync owns, so nothing was written.
    pub directory_synced: u64,
    /// Phones added (or to add).
    pub phones: u64,
    /// Phones not added: the person has one here already.
    pub phones_kept: u64,
}

impl ProfileTally {
    pub fn add(&mut self, o: &ProfileTally) {
        self.organizations += o.organizations;
        self.organizations_kept += o.organizations_kept;
        self.directory_synced += o.directory_synced;
        self.phones += o.phones;
        self.phones_kept += o.phones_kept;
    }
}

/// Decide, without writing, what happens to everyone in `groups`.
pub fn plan(
    conn: &mut DbConnection,
    groups: &[AddressGroup],
    opts: &PeopleOptions<'_>,
) -> QueryResult<PeoplePlan> {
    let facts = facts(conn, groups, opts)?;
    let mut decisions = Vec::new();
    let mut existing: Vec<(Uuid, &AddressGroup)> = Vec::new();
    let mut fresh: Vec<&AddressGroup> = Vec::new();
    for g in groups {
        let group_mapped = g
            .members
            .iter()
            .find_map(|m| facts.mapped.get(&m.source_id));
        let (outcome, target_email) = match group_mapped {
            Some(_) => (Outcome::Mapped, None),
            None => decide(&g.rep, &facts, opts),
        };
        for m in &g.members {
            let outcome = if facts.mapped.contains_key(&m.source_id) {
                Outcome::Mapped
            } else {
                outcome
            };
            decisions.push(Decision {
                person: m.clone(),
                outcome,
                target_email: target_email.clone(),
            });
        }
        if outcome.on_placeholder() || !needs_profile(g, &facts) {
            continue;
        }
        let account = group_mapped.copied().or_else(|| {
            target_email
                .as_deref()
                .and_then(|t| facts.holders.get(&t.to_lowercase()))
                .filter(|_| {
                    matches!(
                        outcome,
                        Outcome::Member | Outcome::Known | Outcome::Reused | Outcome::AgentMapped
                    )
                })
                .map(|h| h.uuid)
        });
        match account {
            Some(uuid) => existing.push((uuid, g)),
            None => fresh.push(g),
        }
    }

    // What the profile fill would write: everything, on a new account; on
    // an existing one, what is empty there.
    let mut profile = ProfileTally::default();
    for g in fresh {
        profile.organizations += u64::from(g.rep.organization.is_some());
        profile.phones += u64::from(g.rep.phone.is_some());
    }
    let uuids: Vec<Uuid> = existing.iter().map(|(u, _)| *u).collect();
    let standing = user_contact::contact_standing(conn, &uuids)?;
    for (uuid, g) in existing {
        let s = standing.get(&uuid).cloned().unwrap_or_default();
        let (org, phone) = (g.rep.organization.is_some(), g.rep.phone.is_some());
        if s.directory_synced {
            profile.directory_synced += u64::from(org || phone);
            continue;
        }
        if org {
            if s.organization.is_some() {
                profile.organizations_kept += 1;
            } else {
                profile.organizations += 1;
            }
        }
        if phone {
            if s.has_phone {
                profile.phones_kept += 1;
            } else {
                profile.phones += 1;
            }
        }
    }

    let mut warnings = Vec::new();
    let mut entries: Vec<(&String, &String)> = opts.agent_map.iter().collect();
    entries.sort();
    for (from, to) in entries {
        let named = groups.iter().flat_map(|g| &g.members).any(|p| {
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
        profile,
        changed_at_source: changed_at_source(groups, &facts),
    })
}

/// The group's profile fill is still to do: someone in it alive at the
/// source has none recorded.
fn needs_profile(g: &AddressGroup, facts: &Facts) -> bool {
    !g.rep.deleted
        && (g.rep.organization.is_some() || g.rep.phone.is_some())
        && g.members
            .iter()
            .any(|m| !m.deleted && !facts.profiled.contains(&m.source_id))
}

/// People brought across before whom the source has changed since: listed,
/// never applied (a primary address decides who signs in, and an address
/// changed at the source is no proof).
fn changed_at_source(groups: &[AddressGroup], facts: &Facts) -> Vec<String> {
    let mut out = Vec::new();
    for p in groups.iter().flat_map(|g| &g.members) {
        let Some(stored) = facts.mapped_updated.get(&p.source_id) else {
            continue;
        };
        let newer = match (p.updated_at, stored) {
            (Some(now), Some(then)) => now.naive_utc() > *then,
            _ => false,
        };
        if newer {
            out.push(format!("{} (source id {})", display_name(p), p.source_id));
        }
    }
    out
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
    if p.deleted {
        return decide_deleted(p, facts, opts);
    }
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
        // Limited staff had partial access at the source: only the agent
        // map seats them.
        None if staff
            && opts.create_agents
            && p.active
            && !p.limited_staff
            && !workspaces::staff_identity_externally_managed("agent") =>
        {
            (Outcome::AgentCreated, Some(email.to_string()))
        }
        None if staff => (Outcome::FormerStaff, None),
        None if !p.active => (Outcome::Inactive, None),
        None => (Outcome::Requester, None),
    }
}

/// Someone deleted at the source keeps their history under one of the
/// workspace's people who holds their address (or the staff member the
/// operator mapped them to); otherwise it goes to the placeholder. Their
/// address never makes or reaches anyone else's account.
fn decide_deleted(
    p: &Person,
    facts: &Facts,
    opts: &PeopleOptions<'_>,
) -> (Outcome, Option<String>) {
    let Some(email) = p.email.as_deref().map(str::to_lowercase) else {
        return (Outcome::Deleted, None);
    };
    if p.role.is_staff() {
        if let Some(target) = opts.agent_map.get(&email) {
            if is_staff_holder(facts.holders.get(&target.to_lowercase())) {
                return (Outcome::AgentMapped, Some(target.clone()));
            }
        }
    }
    match facts.holders.get(&email) {
        Some(h) if h.trusted && h.active_role.is_some() => (Outcome::Member, Some(email)),
        Some(h) if h.trusted && h.person => (Outcome::Known, Some(email)),
        _ => (Outcome::Deleted, None),
    }
}

/// What [`apply`] did.
#[derive(Debug, Default)]
pub struct Applied {
    pub resolved: HashMap<String, Resolved>,
    /// People who couldn't be brought across as decided, what happened
    /// instead, and why.
    pub problems: Vec<String>,
    pub profile: ProfileTally,
    /// People whose organization or phone couldn't be written, and why.
    /// They were brought across all the same.
    pub profile_problems: Vec<String>,
    /// People brought across before whom the source has changed since,
    /// left as they are.
    pub changed_at_source: Vec<String>,
}

/// Bring `groups` across: find or create each account, record it on the
/// id map for everyone in the group, and fill the account's profile here.
/// Decides again from the workspace as it is now, so a plan that went
/// stale can't attach anyone it shouldn't. Runs in the caller's
/// transaction, under its import actor and the source's lock;
/// `placeholder` is [`placeholder`]'s answer. Each group is written in a
/// savepoint: one that fails (a full seat limit, say) falls back to an
/// account without a seat, or to the placeholder, and is reported, and the
/// rest carry on. The profile fill has a savepoint of its own: one that
/// fails is reported and doesn't undo the person.
pub fn apply(
    conn: &mut DbConnection,
    groups: &[AddressGroup],
    opts: &PeopleOptions<'_>,
    placeholder: Uuid,
) -> QueryResult<Applied> {
    let ws = opts.scope.workspace_id;
    let facts = facts(conn, groups, opts)?;
    // A mapping whose account is no longer one of the workspace's people
    // (purged, or never was) is forgotten with its profile and phone
    // entries, and the person brought across again.
    let ids: Vec<String> = groups
        .iter()
        .flat_map(|g| g.members.iter().map(|p| p.source_id.clone()))
        .collect();
    let mut facts = facts;
    for id in idmap::targets_of(conn, opts.scope, Entity::Person, &ids)?.keys() {
        if !facts.mapped.contains_key(id) {
            idmap::forget_person(conn, opts.scope, id)?;
            facts.profiled.remove(id);
        }
    }

    let mut out = Applied {
        changed_at_source: changed_at_source(groups, &facts),
        ..Default::default()
    };
    for g in groups {
        let group_mapped = g
            .members
            .iter()
            .find_map(|m| facts.mapped.get(&m.source_id).copied());
        let unmapped: Vec<&Person> = g
            .members
            .iter()
            .filter(|m| !facts.mapped.contains_key(&m.source_id))
            .collect();
        let group = match group_mapped {
            Some(uuid) => {
                let r = standing(conn, ws, uuid, Outcome::Mapped)?;
                // Others at the same address join the account it went to.
                if !unmapped.is_empty() {
                    let joined = conn.transaction(|conn| {
                        unmapped
                            .iter()
                            .try_for_each(|m| remember(conn, m, &r, opts))
                    });
                    if let Err(e) = joined {
                        out.problems.push(format!(
                            "{}: not recorded with the others at its address ({e})",
                            display_name(&g.rep)
                        ));
                    }
                }
                r
            }
            None => {
                let (outcome, target) = decide(&g.rep, &facts, opts);
                let first = conn.transaction(|conn| {
                    let r = bring(
                        conn,
                        &g.rep,
                        outcome,
                        target.as_deref(),
                        &facts,
                        opts,
                        placeholder,
                    )?;
                    for m in &unmapped {
                        remember(conn, m, &r, opts)?;
                    }
                    Ok::<_, diesel::result::Error>(r)
                });
                match first {
                    Ok(r) => r,
                    Err(e) => {
                        let instead = fallback(conn, &g.rep, &unmapped, opts, placeholder);
                        out.problems.push(format!(
                            "{}: {} ({e}), so {}",
                            display_name(&g.rep),
                            outcome.label(),
                            if instead.outcome.on_placeholder() {
                                "credited to the placeholder author"
                            } else {
                                "brought across without a seat"
                            }
                        ));
                        instead
                    }
                }
            }
        };

        if !group.outcome.on_placeholder() && needs_profile(g, &facts) {
            match conn.transaction(|conn| fill_profile(conn, g, group.uuid, &facts, opts)) {
                Ok(tally) => out.profile.add(&tally),
                Err(e) => out.profile_problems.push(format!(
                    "{}: organization and phone not written ({e})",
                    display_name(&g.rep)
                )),
            }
        }

        for m in &g.members {
            let mut r = match facts.mapped.get(&m.source_id) {
                Some(uuid) if Some(*uuid) != group_mapped => {
                    standing(conn, ws, *uuid, Outcome::Mapped)?
                }
                _ => group.clone(),
            };
            r.source_staff = m.role.is_staff();
            r.source_name = display_name(m);
            // Nothing of a deleted person's but their name.
            r.source_email = m.email.clone().filter(|_| !m.deleted);
            out.resolved.insert(m.source_id.clone(), r);
        }
    }
    Ok(out)
}

/// The organization and phone the source had for the group, into its
/// account's profile here where empty, recorded on the id map for everyone
/// in it alive at the source, so it is done once.
fn fill_profile(
    conn: &mut DbConnection,
    g: &AddressGroup,
    account: Uuid,
    facts: &Facts,
    opts: &PeopleOptions<'_>,
) -> QueryResult<ProfileTally> {
    let mut tally = ProfileTally::default();
    let synced = user_contact::contact_standing(conn, &[account])?
        .get(&account)
        .is_some_and(|s| s.directory_synced);
    if synced {
        // A directory sync owns it and would overwrite it anyway.
        tally.directory_synced += 1;
    } else {
        if let Some(org) = g.rep.organization.as_deref() {
            let org: String = org.trim().chars().take(255).collect();
            match user_contact::fill_organization_if_unset(conn, account, &org)? {
                OrganizationFill::Filled => tally.organizations += 1,
                OrganizationFill::AlreadySet => tally.organizations_kept += 1,
                OrganizationFill::DirectorySynced => tally.directory_synced += 1,
            }
        }
        if let Some(phone) = g.rep.phone.as_deref() {
            match user_contact::add_phone_if_none(conn, account, phone)? {
                Some(row) => {
                    tally.phones += 1;
                    idmap::record(
                        conn,
                        opts.scope,
                        Entity::Phone,
                        &g.rep.source_id,
                        &row.to_string(),
                        opts.job_id,
                        None,
                    )?;
                }
                None => tally.phones_kept += 1,
            }
        }
    }
    for m in &g.members {
        if m.deleted || facts.profiled.contains(&m.source_id) {
            continue;
        }
        idmap::record(
            conn,
            opts.scope,
            Entity::Profile,
            &m.source_id,
            &account.to_string(),
            opts.job_id,
            None,
        )?;
    }
    Ok(tally)
}

/// Where a group goes when bringing it across as decided failed: staff keep
/// their name on a new account without a seat, when no account holds their
/// address in any case (read again now, not from the batch's facts);
/// anyone else, or an address taken, goes to the placeholder. `unmapped`
/// are recorded to the account made.
pub fn fallback(
    conn: &mut DbConnection,
    p: &Person,
    unmapped: &[&Person],
    opts: &PeopleOptions<'_>,
    placeholder: Uuid,
) -> Resolved {
    let withheld = || Resolved::new(placeholder, false, false, Outcome::Withheld);
    if p.deleted {
        return Resolved::new(placeholder, false, false, Outcome::Deleted);
    }
    if !p.role.is_staff() {
        return withheld();
    }
    let Some(email) = p.email.as_deref().map(str::to_lowercase) else {
        return withheld();
    };
    conn.transaction(|conn| {
        let held = user_helpers::address_holders(
            conn,
            opts.scope.workspace_id,
            std::slice::from_ref(&email),
        )?;
        if !held.is_empty() {
            return Err(diesel::result::Error::RollbackTransaction);
        }
        let uuid = create_account(conn, &display_name(p), &email, Seat::None)?;
        let r = Resolved::new(uuid, false, false, Outcome::FormerStaff);
        for m in unmapped {
            remember(conn, m, &r, opts)?;
        }
        Ok(r)
    })
    .unwrap_or_else(|_| withheld())
}

/// Record `r` on the id map, unless it stands for the placeholder.
fn remember(
    conn: &mut DbConnection,
    p: &Person,
    r: &Resolved,
    opts: &PeopleOptions<'_>,
) -> QueryResult<()> {
    if r.outcome.on_placeholder() {
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
        (Outcome::Placeholder | Outcome::Deleted, _) => {
            Ok(Resolved::new(placeholder, false, false, outcome))
        }
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
            deleted: false,
            limited_staff: false,
            custom_role: false,
            organization: None,
            organization_id: None,
            phone: None,
            shared_phone: false,
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
            mapped_updated: HashMap::new(),
            profiled: HashSet::new(),
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
    fn limited_staff_never_agent_created() {
        let map = HashMap::new();
        let f = facts(vec![], &[]);
        let mut light = person(Some("light@x.com"), PersonRole::Agent, true);
        light.limited_staff = true;
        assert_eq!(
            decide(&light, &f, &opts(&map, true)).0,
            Outcome::FormerStaff
        );
        // The operator's map is the way they get a seat.
        let u = Uuid::now_v7();
        let map = HashMap::from([("light@x.com".to_string(), "b@here.com".to_string())]);
        let staff = facts(
            vec![("b@here.com", holder(u, true, Some("agent"), true))],
            &[],
        );
        assert_eq!(
            decide(&light, &staff, &opts(&map, true)).0,
            Outcome::AgentMapped
        );
    }

    #[test]
    fn a_deleted_person_keeps_history_only_with_someone_here() {
        let map = HashMap::new();
        let o = opts(&map, true);
        let mut gone = person(Some("d@x.com"), PersonRole::Requester, false);
        gone.deleted = true;
        assert_eq!(decide(&gone, &facts(vec![], &[]), &o).0, Outcome::Deleted);
        let u = Uuid::now_v7();
        // An account from elsewhere is never reached on a deleted person's word.
        let elsewhere = facts(vec![("d@x.com", holder(u, true, None, false))], &[u]);
        assert_eq!(decide(&gone, &elsewhere, &o).0, Outcome::Deleted);
        let ours = facts(
            vec![("d@x.com", holder(u, true, Some("member"), true))],
            &[],
        );
        assert_eq!(
            decide(&gone, &ours, &o),
            (Outcome::Member, Some("d@x.com".into()))
        );
        let known = facts(vec![("d@x.com", holder(u, true, None, true))], &[]);
        assert_eq!(decide(&gone, &known, &o).0, Outcome::Known);
        gone.email = None;
        assert_eq!(decide(&gone, &facts(vec![], &[]), &o).0, Outcome::Deleted);
    }

    fn member(id: &str, role: PersonRole, created: Option<u32>) -> Person {
        use chrono::TimeZone;
        Person {
            source_id: id.into(),
            name: format!("Name {id}"),
            created_at: created.map(|d| {
                chrono::Utc
                    .with_ymd_and_hms(2024, 1, d, 0, 0, 0)
                    .single()
                    .expect("date")
            }),
            organization: Some(format!("Org {id}")),
            ..person(Some("Shared@X.com"), role, true)
        }
    }

    #[test]
    fn address_group_takes_least_privilege() {
        let mut deleted_requester = member("1", PersonRole::Requester, Some(1));
        deleted_requester.deleted = true;
        deleted_requester.active = false;
        let mut suspended_agent = member("2", PersonRole::Agent, Some(2));
        suspended_agent.active = false;
        let admin = member("3", PersonRole::Admin, Some(3));
        let requester = member("4", PersonRole::Requester, Some(4));
        let alone = Person {
            email: Some("alone@x.com".into()),
            ..member("5", PersonRole::Admin, None)
        };
        let groups = group_by_address(vec![
            deleted_requester.clone(),
            suspended_agent.clone(),
            alone,
            admin.clone(),
            requester,
        ]);
        assert_eq!(groups.len(), 2);
        let g = &groups[0];
        assert_eq!(g.members.len(), 4);
        assert!(g.shared());
        // Any end user makes it a requester; seated, as one is active.
        assert_eq!(g.rep.role, PersonRole::Requester);
        assert!(g.rep.active && !g.rep.deleted);
        // Named for the earliest-created active member, the deleted one
        // counting for attribution only.
        assert_eq!(g.rep.name, "Name 3");
        assert_eq!(g.rep.source_id, "3");
        assert_eq!(g.rep.organization.as_deref(), Some("Org 3"));
        let map = HashMap::new();
        // Never an agent, never created as one.
        assert_eq!(
            decide(&g.rep, &facts(vec![], &[]), &opts(&map, true)).0,
            Outcome::Requester
        );

        // Staff alone: the least of them, and limited if any is.
        let mut light = member("6", PersonRole::Agent, Some(6));
        light.limited_staff = true;
        let groups = group_by_address(vec![admin, light]);
        assert_eq!(groups[0].rep.role, PersonRole::Agent);
        assert!(groups[0].rep.limited_staff);
        assert_eq!(
            decide(&groups[0].rep, &facts(vec![], &[]), &opts(&map, true)).0,
            Outcome::FormerStaff
        );

        // Everyone deleted: attribution only.
        let groups = group_by_address(vec![deleted_requester]);
        assert!(groups[0].rep.deleted);
        assert_eq!(
            decide(&groups[0].rep, &facts(vec![], &[]), &opts(&map, true)).0,
            Outcome::Deleted
        );
        // Not deleted, but nobody active: an account without a seat.
        let groups = group_by_address(vec![suspended_agent]);
        assert!(!groups[0].rep.active);
    }

    #[test]
    fn placeholder_addresses_are_undeliverable_and_unguessable() {
        let a = placeholder_email();
        assert!(a.ends_with("@import.invalid"));
        assert_ne!(a, placeholder_email());
    }
}
