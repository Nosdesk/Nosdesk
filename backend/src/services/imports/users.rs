//! User CSV importer.
//!
//! Natural key: `email`, matched against primary emails. The import
//! only ever touches this workspace's members: a row whose email
//! belongs to a member updates them, a row with a new email creates an
//! account and its membership, and a row whose email belongs to an
//! account outside the workspace is refused with the add-user API's
//! "already exists" error. Auth identities are out of scope for the
//! import path. Bulk-imported users get a passwordless row that admins
//! later associate with an identity provider, send a reset link to, or
//! invite to set up MFA.
//!
//! The import mirrors the API's gates. A workspace role, on a new or an
//! existing member, passes the member-role endpoint's gate (the importer's
//! tier, the target's tier, hosted seat authority, the last owner) or the
//! row is refused in that endpoint's words; an unchanged role isn't gated.
//! An update never changes the member's platform role. Their name and pronouns
//! change only where the edit-user API would let the importing admin
//! change them (their own row, or anyone's as a platform admin), and a
//! name owned by a Nosdesk account stays put. `audit_reviewer` creates
//! as the add-user API does (self-hosted only) and can't be granted to
//! an existing member.
//!
//! A member whose row would change nothing is Unchanged: nothing is
//! written for them. On a member's row a blank cell leaves the value as it
//! is, so a blank role or name is no change, never a refusal.
//!
//! Columns:
//!   - email   (required, the natural key)
//!   - name    (required for a new person; or First name and Last name)
//!   - first_name, last_name (optional): joined with a space into the name
//!     on a row whose Name cell is blank, so a file with a Name column and
//!     First and Last columns takes Name where it has one
//!   - role    (required for a new person: admin | technician | user |
//!     audit_reviewer)
//!   - pronouns (optional)

use std::collections::{HashMap, HashSet};

use diesel::OptionalExtension;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::errors::EXTERNALLY_MANAGED_MESSAGE;
use crate::handlers::workspace_members::{
    can_assign, can_manage, FORBIDDEN_TIER_MESSAGE, LAST_OWNER_MESSAGE, SEAT_LIMIT_MESSAGE,
};
use crate::models::{NewUser, PlatformRole, User, UserUpdate, WorkspaceRole};
use crate::repository::user_helpers::{
    create_user_with_email, primary_email_holders, CreateUserWithEmailOutcome, PrimaryEmailHolder,
};
use crate::repository::users::update_user;
use crate::repository::workspaces::{
    count_workspace_owners, is_seat_limit_violation, membership, nosdesk_account_managed,
    update_membership_role, SeatWriteAuthority, UpdateMembershipRoleResult,
};

use super::csv_parser::ParsedCsv;
use super::types::{
    duplicate_in_file, CommitError, Committed, FieldChange, FieldGroup, FieldSpec, ImportContext,
    ImportOptions, ImportType, ImportedRecords, ImportedUser, Importer, RecordId, RowOutcome,
    RowPlan, RowResult, ValueType,
};
use super::values::Choice;

const HEADERS: &[&str] = &["email", "name", "role", "pronouns"];

const AUDIT_REVIEWER_REFUSED: &str =
    "audit_reviewer can't be set by import; set it from the user's page";

pub struct UserImporter;

impl Importer for UserImporter {
    fn template_headers(&self) -> &'static [&'static str] {
        HEADERS
    }

    fn fields(&self, _conn: &mut DbConnection) -> Result<Vec<FieldSpec>, diesel::result::Error> {
        let t = ImportType::Users;
        Ok(vec![
            FieldSpec::fixed(
                t,
                "email",
                "Email",
                FieldGroup::Core,
                ValueType::Email,
                true,
            ),
            FieldSpec::fixed(t, "name", "Name", FieldGroup::Core, ValueType::Text, true),
            FieldSpec::fixed(
                t,
                "first_name",
                "First name",
                FieldGroup::Core,
                ValueType::Text,
                false,
            ),
            FieldSpec::fixed(
                t,
                "last_name",
                "Last name",
                FieldGroup::Core,
                ValueType::Text,
                false,
            ),
            FieldSpec::fixed(t, "role", "Role", FieldGroup::Core, ValueType::Enum, true)
                .with_enum(["admin", "technician", "user"].map(String::from).to_vec()),
            FieldSpec::fixed(
                t,
                "pronouns",
                "Pronouns",
                FieldGroup::Core,
                ValueType::Text,
                false,
            ),
        ])
    }

    fn value_choices(
        &self,
        _conn: &mut DbConnection,
        field: &FieldSpec,
    ) -> Result<Option<Vec<Choice>>, diesel::result::Error> {
        Ok((field.id == "role").then(|| {
            vec![
                Choice::new("admin", "Admin"),
                Choice::new("technician", "Technician"),
                Choice::new("user", "User"),
                Choice::new("audit_reviewer", "Audit reviewer"),
            ]
        }))
    }

    fn value_synonyms(&self, field: &str) -> &'static [(&'static str, &'static str)] {
        match field {
            "role" => &[
                ("administrator", "admin"),
                ("agent", "technician"),
                ("tech", "technician"),
                ("support", "technician"),
                ("member", "user"),
                ("requester", "user"),
                ("end user", "user"),
                ("staff", "user"),
            ],
            _ => &[],
        }
    }

    fn synonyms(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("emailaddress", "email"),
            ("mail", "email"),
            ("fullname", "name"),
            ("displayname", "name"),
            ("first", "first_name"),
            ("givenname", "first_name"),
            ("forename", "first_name"),
            ("last", "last_name"),
            ("surname", "last_name"),
            ("familyname", "last_name"),
            ("userrole", "role"),
        ]
    }

    fn dry_run(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        ctx: &ImportContext,
    ) -> Result<RowPlan, diesel::result::Error> {
        if let Some(err) = check_headers(&parsed.headers) {
            return Ok(RowPlan {
                file_error: Some(err),
                ..Default::default()
            });
        }

        let lookup = Lookup::load(conn, parsed, ctx)?;
        let mut state = FileState::new(&lookup, parsed);
        let mut plan = RowPlan::default();

        for record in parsed.records() {
            let row_num = record.source_row;
            let values = with_name(lookup.with_constants(record.values, &ctx.options));
            let mut local = state.clone();
            let result = match validate_row(&values, &lookup, &mut local) {
                Ok(action) => {
                    state = local;
                    match action {
                        RowAction::Create { .. } => RowResult::planned(row_num, RowOutcome::Create),
                        RowAction::UpdateMember {
                            user_uuid, changes, ..
                        } => {
                            let mut result = RowResult::planned(row_num, RowOutcome::Update)
                                .with_record(RecordId::Uuid(user_uuid));
                            result.fields_written = changes;
                            result
                        }
                        RowAction::Unchanged(user_uuid) => {
                            RowResult::planned(row_num, RowOutcome::Unchanged)
                                .with_record(RecordId::Uuid(user_uuid))
                        }
                    }
                }
                Err(errs) => RowResult::skipped(row_num, errs),
            };
            plan.rows.push(identify(result, &values));
        }
        Ok(plan)
    }

    fn commit(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        ctx: &ImportContext,
    ) -> Result<Committed, CommitError> {
        if check_headers(&parsed.headers).is_some() {
            return Err(diesel::result::Error::QueryBuilderError(
                "header validation should have caught this; refusing to commit".into(),
            )
            .into());
        }
        let lookup = Lookup::load(conn, parsed, ctx)?;
        let mut state = FileState::new(&lookup, parsed);

        let mut imported: Vec<ImportedUser> = Vec::new();
        let mut rows = Vec::with_capacity(parsed.rows.len());
        for record in parsed.records() {
            let row_num = record.source_row;
            let values = with_name(lookup.with_constants(record.values, &ctx.options));
            let row = &values;
            // A write refused below, for a reason validation can't see,
            // skips the row with the member-role endpoint's words.
            let refused = |message: &str| {
                identify(
                    RowResult::skipped(row_num, vec![(Some("role".into()), message.to_string())]),
                    row,
                )
            };
            let mut local = state.clone();
            let action = match validate_row(row, &lookup, &mut local) {
                Ok(a) => a,
                Err(errs) => {
                    rows.push(identify(RowResult::skipped(row_num, errs), row));
                    continue;
                }
            };
            state = local;
            // A seat-limit refusal from the database aborts the transaction;
            // report it against the row, in the member-role endpoint's words.
            let seat_limit = |e: diesel::result::Error| -> CommitError {
                if is_seat_limit_violation(&e) {
                    CommitError::Row {
                        row: record.source_row,
                        column: Some("role".to_string()),
                        message: SEAT_LIMIT_MESSAGE.to_string(),
                    }
                } else {
                    e.into()
                }
            };

            let email = trimmed(row, "email");
            match action {
                RowAction::Create {
                    platform_role,
                    workspace_role,
                } => {
                    let new_user = NewUser {
                        uuid: Uuid::new_v4(),
                        name: trimmed(row, "name"),
                        pronouns: opt_string(row, "pronouns"),
                        avatar_url: None,
                        banner_url: None,
                        avatar_thumb: None,
                        microsoft_uuid: None,
                        mfa_secret: None,
                        mfa_secret_kek_id: None,
                        mfa_enabled: false,
                        platform_role: Some(platform_role.as_str().to_string()),
                    };
                    // create_user_with_email writes the user, the primary
                    // email, and the workspace_members row (workspace from
                    // app.workspace_id, pinned by TenantConn) and emits
                    // user.created, the same path the create-user API uses.
                    // observer = None: the import handler indexes every
                    // committed entity post-commit, so all three importers
                    // index uniformly without per-importer observer wiring.
                    match create_user_with_email(
                        new_user,
                        workspace_role,
                        email,
                        false, // email starts unverified; admin / IdP verifies later
                        Some("csv_import".to_string()),
                        conn,
                        None,
                        SeatWriteAuthority::Product,
                    )
                    .map_err(seat_limit)?
                    {
                        CreateUserWithEmailOutcome::Created(user, user_email) => {
                            rows.push(identify(
                                RowResult::planned(row_num, RowOutcome::Create)
                                    .with_record(RecordId::Uuid(user.uuid)),
                                row,
                            ));
                            imported.push(ImportedUser {
                                user,
                                primary_email: Some(user_email.email),
                            });
                        }
                        // Refused: a staff seat in hosted is control-plane-owned.
                        // Validation blocks this upstream, so this is a defensive
                        // skip rather than an import.
                        CreateUserWithEmailOutcome::ExternallyManaged => {
                            rows.push(refused(EXTERNALLY_MANAGED_MESSAGE));
                            continue;
                        }
                    }
                }
                RowAction::Unchanged(user_uuid) => {
                    // Nothing to write: no update, no event.
                    rows.push(identify(
                        RowResult::planned(row_num, RowOutcome::Unchanged)
                            .with_record(RecordId::Uuid(user_uuid)),
                        row,
                    ));
                }
                RowAction::UpdateMember {
                    user_uuid,
                    new_role,
                    name,
                    pronouns,
                    changes,
                } => {
                    // A role change passed the member-role endpoint's gate in
                    // validate_row; update_membership_role (Product authority)
                    // re-checks hosted seats and the last owner, and a refusal
                    // here skips the row. Role changes are operator-side and
                    // intentionally not sync-emitted.
                    if let Some(role) = new_role {
                        match update_membership_role(
                            conn,
                            lookup.workspace_id,
                            user_uuid,
                            role.as_str(),
                            SeatWriteAuthority::Product,
                        )
                        .map_err(seat_limit)?
                        {
                            UpdateMembershipRoleResult::Updated(_) => {}
                            UpdateMembershipRoleResult::NotFound => {
                                rows.push(refused("this person is no longer a member"));
                                continue;
                            }
                            UpdateMembershipRoleResult::LastOwner => {
                                rows.push(refused(LAST_OWNER_MESSAGE));
                                continue;
                            }
                            UpdateMembershipRoleResult::ExternallyManaged => {
                                rows.push(refused(EXTERNALLY_MANAGED_MESSAGE));
                                continue;
                            }
                        }
                    }
                    // update_user emits user.updated (carrying the new
                    // workspace role) and returns the refreshed model for
                    // indexing. Name and pronouns are set only when the
                    // validation above allowed the change.
                    let user = update_user(
                        &user_uuid,
                        UserUpdate {
                            name,
                            pronouns,
                            avatar_url: None,
                            banner_url: None,
                            avatar_thumb: None,
                            microsoft_uuid: None,
                            updated_at: Some(chrono::Utc::now().naive_utc()),
                        },
                        conn,
                        None,
                    )?;
                    let mut result = RowResult::planned(row_num, RowOutcome::Update)
                        .with_record(RecordId::Uuid(user.uuid));
                    result.fields_written = changes;
                    rows.push(identify(result, row));
                    imported.push(ImportedUser {
                        user,
                        primary_email: Some(email),
                    });
                }
            }
        }
        Ok(Committed {
            records: ImportedRecords::Users(imported),
            rows,
            created: Vec::new(),
        })
    }
}

#[derive(Debug, Clone)]
enum RowAction {
    Create {
        platform_role: PlatformRole,
        workspace_role: WorkspaceRole,
    },
    /// `new_role` / `name` / `pronouns` are `Some` only when they change;
    /// at least one does. `changes` is each with its value before and after.
    UpdateMember {
        user_uuid: Uuid,
        new_role: Option<WorkspaceRole>,
        name: Option<String>,
        pronouns: Option<String>,
        changes: std::collections::BTreeMap<String, FieldChange>,
    },
    /// A member whose row changes nothing.
    Unchanged(Uuid),
}

/// What the rows are checked against: who holds each email in the file,
/// and what the importing admin may change.
struct Lookup {
    workspace_id: i32,
    holders: HashMap<String, PrimaryEmailHolder>,
    /// The importing admin.
    actor: Option<Uuid>,
    /// Whether the importing admin is a platform admin, who may edit
    /// anyone's profile through the edit-user API.
    actor_is_platform_admin: bool,
    /// Members whose name is owned by their Nosdesk account (hosted staff).
    name_managed: HashSet<Uuid>,
    /// The importing admin's active role in the workspace, the tier the
    /// member-role endpoint gates on.
    actor_role: Option<WorkspaceRole>,
    /// Active owners before the import.
    owners: i64,
}

/// What the file as a whole holds and earlier rows have claimed. Cloned per
/// row and kept only when the row validates, so a refused row claims
/// nothing.
#[derive(Clone)]
struct FileState {
    /// Emails (lowercased) more than one row holds, each with its rows.
    /// Every one of those rows is skipped.
    duplicate_emails: std::rc::Rc<HashMap<String, Vec<usize>>>,
    /// Owners left after the owner demotions accepted so far.
    owners: i64,
}

impl FileState {
    fn new(lookup: &Lookup, parsed: &ParsedCsv) -> Self {
        let mut emails: HashMap<String, Vec<usize>> = HashMap::new();
        for record in parsed.records() {
            let email = trimmed(&record.values, "email").to_lowercase();
            if !email.is_empty() {
                emails.entry(email).or_default().push(record.source_row);
            }
        }
        emails.retain(|_, rows| rows.len() > 1);
        Self {
            duplicate_emails: std::rc::Rc::new(emails),
            owners: lookup.owners,
        }
    }
}

/// The row's email and name on its result.
fn identify(result: RowResult, row: &HashMap<String, String>) -> RowResult {
    result
        .with_key(Some(trimmed(row, "email")))
        .with_label(trimmed(row, "name"))
}

/// The row with its name settled: the Name cell, or, when that is blank,
/// the First name and Last name cells joined with a space.
fn with_name(mut row: HashMap<String, String>) -> HashMap<String, String> {
    if trimmed(&row, "name").is_empty() {
        let joined = [trimmed(&row, "first_name"), trimmed(&row, "last_name")]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        row.insert("name".to_string(), joined);
    }
    row
}

impl Lookup {
    /// The row with the job's constants as the person takes them: a
    /// constant never touches an existing member, so their row drops them.
    fn with_constants(
        &self,
        mut row: HashMap<String, String>,
        options: &ImportOptions,
    ) -> HashMap<String, String> {
        if options.constants.is_empty() {
            return row;
        }
        let email = trimmed(&row, "email").to_lowercase();
        if let Some(PrimaryEmailHolder::Member { .. }) = self.holders.get(&email) {
            for constant in &options.constants {
                if let Some(cell) = row.get_mut(&constant.field) {
                    cell.clear();
                }
            }
        }
        row
    }

    fn load(
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        ctx: &ImportContext,
    ) -> Result<Self, diesel::result::Error> {
        let workspace_id = crate::sync::session::current_workspace_id(conn)?.ok_or_else(|| {
            diesel::result::Error::QueryBuilderError("user import needs a pinned workspace".into())
        })?;
        let emails: Vec<String> = parsed
            .records()
            .map(|r| trimmed(&r.values, "email"))
            .filter(|e| !e.is_empty())
            .collect();
        let holders = primary_email_holders(conn, workspace_id, &emails)?;
        let actor_is_platform_admin = match ctx.actor {
            Some(actor) => crate::repository::users::get_user_by_uuid(&actor, conn)
                .optional()?
                .is_some_and(|u| PlatformRole::from_db(&u.platform_role).is_platform_admin()),
            None => false,
        };
        let member_uuids: Vec<Uuid> = holders
            .values()
            .filter_map(|h| match h {
                PrimaryEmailHolder::Member { user, .. } => Some(user.uuid),
                PrimaryEmailHolder::NotMember => None,
            })
            .collect();
        let name_managed = nosdesk_account_managed(conn, &member_uuids);
        let actor_role = match ctx.actor {
            Some(actor) => {
                membership(conn, workspace_id, actor)?.map(|m| WorkspaceRole::from_db(&m.role))
            }
            None => None,
        };
        let owners = count_workspace_owners(conn, workspace_id)?;
        Ok(Self {
            workspace_id,
            holders,
            actor: ctx.actor,
            actor_is_platform_admin,
            name_managed,
            actor_role,
            owners,
        })
    }

    /// The edit-user API's rule for another person's profile: your own,
    /// or anyone's as a platform admin.
    fn may_edit_profile(&self, user: &User) -> bool {
        self.actor == Some(user.uuid) || self.actor_is_platform_admin
    }

    /// The member-role endpoint's gate (`PATCH /api/workspace/members/{uuid}`)
    /// for giving `new` to a member whose role is `current` (`None` for a new
    /// member), checked in the endpoint's order and refused in its words.
    fn role_gate(
        &self,
        state: &mut FileState,
        current: Option<&str>,
        new: WorkspaceRole,
    ) -> Result<(), String> {
        let caller = match self.actor_role {
            None => return Err("You are not a member of this workspace".into()),
            Some(r) if !r.meets(WorkspaceRole::Admin) => {
                return Err(format!(
                    "This action requires {} privileges or higher in this workspace",
                    WorkspaceRole::Admin.as_str()
                ))
            }
            Some(r) => r,
        };
        if !can_assign(caller, new) {
            return Err(FORBIDDEN_TIER_MESSAGE.into());
        }
        let Some(current) = current else {
            if SeatWriteAuthority::Product.refuses_role(new.as_str()) {
                return Err(EXTERNALLY_MANAGED_MESSAGE.into());
            }
            return Ok(());
        };
        if !can_manage(caller, WorkspaceRole::from_db(current)) {
            return Err(FORBIDDEN_TIER_MESSAGE.into());
        }
        if SeatWriteAuthority::Product.refuses_change(current, new.as_str()) {
            return Err(EXTERNALLY_MANAGED_MESSAGE.into());
        }
        if current == WorkspaceRole::Owner.as_str() && new != WorkspaceRole::Owner {
            if state.owners <= 1 {
                return Err(LAST_OWNER_MESSAGE.into());
            }
            state.owners -= 1;
        }
        Ok(())
    }
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
    lookup: &Lookup,
    state: &mut FileState,
) -> Result<RowAction, Vec<(Option<String>, String)>> {
    let mut errors: Vec<(Option<String>, String)> = Vec::new();

    let email = trimmed(row, "email");
    if email.is_empty() {
        errors.push((Some("email".into()), "email is required".into()));
    } else if !looks_like_email(&email) {
        errors.push((
            Some("email".into()),
            format!("'{email}' is not a valid email address"),
        ));
    } else if let Some(rows) = state.duplicate_emails.get(&email.to_lowercase()) {
        errors.push((
            Some("email".into()),
            duplicate_in_file("email", &email, rows),
        ));
    }
    // Who the row is, once its email reads: a member's row leaves a blank
    // name or role as it is.
    let holder = if errors.is_empty() {
        lookup.holders.get(&email.to_lowercase())
    } else {
        None
    };
    let member = matches!(holder, Some(PrimaryEmailHolder::Member { .. }));

    let name = trimmed(row, "name");
    if name.is_empty() && !member {
        errors.push((Some("name".into()), "name is required".into()));
    }

    let role_raw = trimmed(row, "role");
    let roles = if role_raw.is_empty() {
        if !member {
            errors.push((Some("role".into()), "role is required".into()));
        }
        None
    } else {
        match crate::utils::parse_roles(&role_raw) {
            Err(_) => {
                errors.push((
                    Some("role".into()),
                    format!("'{role_raw}' is not a valid role; use admin, technician, or user"),
                ));
                None
            }
            Ok(roles) => Some(roles),
        }
    };

    if !errors.is_empty() {
        return Err(errors);
    }

    match holder {
        Some(PrimaryEmailHolder::Member { user, role }) => {
            validate_member(row, lookup, state, user, role, name, roles)
        }
        // An account outside this workspace: refused as the add-user API
        // refuses it, with the same message whatever the account is.
        Some(PrimaryEmailHolder::NotMember) => Err(vec![(
            Some("email".into()),
            "User with this email already exists".into(),
        )]),
        None => {
            let Some((platform_role, workspace_role)) = roles else {
                return Err(vec![(Some("role".into()), "role is required".into())]);
            };
            if let Err(msg) = lookup.role_gate(state, None, workspace_role) {
                return Err(vec![(Some("role".into()), msg)]);
            }
            // The add-user API creates an audit reviewer only where local
            // credentials are allowed (self-hosted), for a workspace admin,
            // which the import already requires.
            if platform_role != PlatformRole::User
                && !crate::middleware::workspace_context::local_credentials_permitted()
            {
                return Err(vec![(Some("role".into()), AUDIT_REVIEWER_REFUSED.into())]);
            }
            Ok(RowAction::Create {
                platform_role,
                workspace_role,
            })
        }
    }
}

/// A member's row: what it changes, each change gated as the API gates it,
/// or Unchanged when it changes nothing. A blank name or role is no change.
fn validate_member(
    row: &HashMap<String, String>,
    lookup: &Lookup,
    state: &mut FileState,
    user: &User,
    role: &str,
    name: String,
    roles: Option<(PlatformRole, WorkspaceRole)>,
) -> Result<RowAction, Vec<(Option<String>, String)>> {
    let mut errors: Vec<(Option<String>, String)> = Vec::new();
    let mut changes = std::collections::BTreeMap::new();

    // An unchanged role writes nothing, so it isn't gated.
    let new_role = roles
        .map(|(_, workspace_role)| workspace_role)
        .filter(|r| role != r.as_str());
    if let Some(new) = new_role {
        if let Err(msg) = lookup.role_gate(state, Some(role), new) {
            errors.push((Some("role".into()), msg));
        }
        changes.insert(
            "role".to_string(),
            FieldChange {
                before: Some(role_name(role).to_string()),
                after: Some(role_name(new.as_str()).to_string()),
            },
        );
    }
    // The platform role is never written on update; a row may only name
    // one the member already holds.
    if let Some((platform_role, _)) = roles {
        if platform_role != PlatformRole::User
            && PlatformRole::from_db(&user.platform_role) != platform_role
        {
            errors.push((Some("role".into()), AUDIT_REVIEWER_REFUSED.into()));
        }
    }

    let name_change = (!name.is_empty() && name != user.name).then_some(name);
    if let Some(new) = &name_change {
        if lookup.name_managed.contains(&user.uuid) {
            errors.push((
                Some("name".into()),
                "this person's name is managed in their Nosdesk account".into(),
            ));
        } else if !lookup.may_edit_profile(user) {
            errors.push((
                Some("name".into()),
                "only the person or a platform admin can change their name".into(),
            ));
        }
        changes.insert(
            "name".to_string(),
            FieldChange {
                before: Some(user.name.clone()),
                after: Some(new.clone()),
            },
        );
    }
    let pronouns_change =
        opt_string(row, "pronouns").filter(|p| user.pronouns.as_deref() != Some(p));
    if let Some(new) = &pronouns_change {
        if !lookup.may_edit_profile(user) {
            errors.push((
                Some("pronouns".into()),
                "only the person or a platform admin can change their pronouns".into(),
            ));
        }
        changes.insert(
            "pronouns".to_string(),
            FieldChange {
                before: user.pronouns.clone(),
                after: Some(new.clone()),
            },
        );
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    if changes.is_empty() {
        return Ok(RowAction::Unchanged(user.uuid));
    }
    Ok(RowAction::UpdateMember {
        user_uuid: user.uuid,
        new_role,
        name: name_change,
        pronouns: pronouns_change,
        changes,
    })
}

/// A workspace role as the import's Role column words it.
fn role_name(db_role: &str) -> &str {
    match db_role {
        "member" => "user",
        "agent" => "technician",
        other => other,
    }
}

/// Pragmatic email shape check. Same constraints as the
/// kind-validator `is_email` (single @, non-empty local +
/// domain, domain has at least one dot, no consecutive dots,
/// no whitespace). Not RFC 5322 complete; close enough to keep
/// hand-typed CSV rows honest.
fn looks_like_email(s: &str) -> bool {
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return false;
    }
    let at = match s.find('@') {
        Some(i) if !s[i + 1..].contains('@') => i,
        _ => return false,
    };
    let (local, rest) = s.split_at(at);
    let domain = &rest[1..];
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return false;
    }
    if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
        return false;
    }
    if domain.contains("..") {
        return false;
    }
    true
}

fn trimmed(row: &HashMap<String, String>, key: &str) -> String {
    row.get(key).cloned().unwrap_or_default().trim().to_string()
}

fn opt_string(row: &HashMap<String, String>, key: &str) -> Option<String> {
    let v = trimmed(row, key);
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}
