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
//! The import mirrors the API's gates. An update changes the member's
//! workspace role, never their platform role. Their name and pronouns
//! change only where the edit-user API would let the importing admin
//! change them (their own row, or anyone's as a platform admin), and a
//! name owned by a Nosdesk account stays put. `audit_reviewer` creates
//! as the add-user API does (self-hosted only) and can't be granted to
//! an existing member.
//!
//! Columns:
//!   - email   (required, the natural key)
//!   - name    (required)
//!   - role    (required: admin | technician | user | audit_reviewer)
//!   - pronouns (optional)

use std::collections::{HashMap, HashSet};

use diesel::OptionalExtension;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewUser, PlatformRole, User, UserUpdate, WorkspaceRole};
use crate::repository::user_helpers::{
    create_user_with_email, primary_email_holders, CreateUserWithEmailOutcome, PrimaryEmailHolder,
};
use crate::repository::users::update_user;
use crate::repository::workspaces::{
    nosdesk_account_managed, staff_change_externally_managed, staff_identity_externally_managed,
    update_membership_role, SeatWriteAuthority, UpdateMembershipRoleResult,
};

use super::csv_parser::ParsedCsv;
use super::types::{
    CommitError, ImportContext, ImportSummary, ImportedRecords, ImportedUser, Importer, RowError,
    MAX_ERRORS,
};

const HEADERS: &[&str] = &["email", "name", "role", "pronouns"];

const STAFF_MANAGED: &str = "team members (admin, technician) are managed in the Nosdesk control plane; import requesters (user) only";
const AUDIT_REVIEWER_REFUSED: &str =
    "audit_reviewer can't be set by import; set it from the user's page";

pub struct UserImporter;

impl Importer for UserImporter {
    fn template_headers(&self) -> &'static [&'static str] {
        HEADERS
    }

    fn dry_run(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        ctx: &ImportContext,
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

        let lookup = Lookup::load(conn, parsed, ctx)?;
        let mut emails_in_file: HashSet<String> = HashSet::new();

        for record in parsed.records() {
            let row_num = record.source_row;
            match validate_row(&record.values, &lookup, &mut emails_in_file) {
                Ok(RowAction::Create { .. }) => summary.would_create += 1,
                Ok(RowAction::UpdateMember { .. }) => summary.would_update += 1,
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
        ctx: &ImportContext,
    ) -> Result<ImportedRecords, CommitError> {
        if check_headers(&parsed.headers).is_some() {
            return Err(diesel::result::Error::QueryBuilderError(
                "header validation should have caught this; refusing to commit".into(),
            )
            .into());
        }
        let lookup = Lookup::load(conn, parsed, ctx)?;
        let mut emails_in_file: HashSet<String> = HashSet::new();

        let mut imported: Vec<ImportedUser> = Vec::new();
        for record in parsed.records() {
            let row = &record.values;
            let mut local = emails_in_file.clone();
            let action = match validate_row(row, &lookup, &mut local) {
                Ok(a) => a,
                Err(_) => continue,
            };
            emails_in_file = local;

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
                    )? {
                        CreateUserWithEmailOutcome::Created(user, user_email) => {
                            imported.push(ImportedUser {
                                user,
                                primary_email: Some(user_email.email),
                            });
                        }
                        // Refused: a staff seat in hosted is control-plane-owned.
                        // Validation blocks this upstream, so this is a defensive
                        // skip rather than an import.
                        CreateUserWithEmailOutcome::ExternallyManaged => continue,
                    }
                }
                RowAction::UpdateMember {
                    user_uuid,
                    workspace_role,
                    name,
                    pronouns,
                } => {
                    // The membership role change routes through the gated
                    // update_membership_role (Product authority): a staff seat
                    // in hosted is control-plane-owned, so it's refused and the
                    // row skipped (validation blocks it upstream; this is the
                    // enforcement point). Role changes are operator-side and
                    // intentionally not sync-emitted.
                    match update_membership_role(
                        conn,
                        lookup.workspace_id,
                        user_uuid,
                        workspace_role.as_str(),
                        SeatWriteAuthority::Product,
                    )? {
                        UpdateMembershipRoleResult::Updated(_)
                        | UpdateMembershipRoleResult::LastOwner => {}
                        UpdateMembershipRoleResult::NotFound
                        | UpdateMembershipRoleResult::ExternallyManaged => continue,
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
                    imported.push(ImportedUser {
                        user,
                        primary_email: Some(email),
                    });
                }
            }
        }
        Ok(ImportedRecords::Users(imported))
    }
}

#[derive(Debug, Clone)]
enum RowAction {
    Create {
        platform_role: PlatformRole,
        workspace_role: WorkspaceRole,
    },
    /// `name` / `pronouns` are `Some` only when they change.
    UpdateMember {
        user_uuid: Uuid,
        workspace_role: WorkspaceRole,
        name: Option<String>,
        pronouns: Option<String>,
    },
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
}

impl Lookup {
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
        Ok(Self {
            workspace_id,
            holders,
            actor: ctx.actor,
            actor_is_platform_admin,
            name_managed,
        })
    }

    /// The edit-user API's rule for another person's profile: your own,
    /// or anyone's as a platform admin.
    fn may_edit_profile(&self, user: &User) -> bool {
        self.actor == Some(user.uuid) || self.actor_is_platform_admin
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
    lookup: &Lookup,
    emails_in_file: &mut HashSet<String>,
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
    }

    let name = trimmed(row, "name");
    if name.is_empty() {
        errors.push((Some("name".into()), "name is required".into()));
    }

    let role_raw = trimmed(row, "role");
    let roles = if role_raw.is_empty() {
        errors.push((Some("role".into()), "role is required".into()));
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
            Ok((_, ws_role)) if staff_identity_externally_managed(ws_role.as_str()) => {
                // In hosted, staff seats are provisioned in the control plane;
                // the import can only create/update requesters.
                errors.push((Some("role".into()), STAFF_MANAGED.into()));
                None
            }
            Ok(roles) => Some(roles),
        }
    };

    let Some((platform_role, workspace_role)) = roles.filter(|_| errors.is_empty()) else {
        return Err(errors);
    };

    let email_key = email.to_lowercase();
    if !emails_in_file.insert(email_key.clone()) {
        return Err(vec![(
            Some("email".into()),
            format!("email '{email}' appears more than once in this file"),
        )]);
    }

    match lookup.holders.get(&email_key) {
        None => {
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
        // An account outside this workspace: refused as the add-user API
        // refuses it, with the same message whatever the account is.
        Some(PrimaryEmailHolder::NotMember) => Err(vec![(
            Some("email".into()),
            "User with this email already exists".into(),
        )]),
        Some(PrimaryEmailHolder::Member { user, role }) => {
            if staff_change_externally_managed(role, workspace_role.as_str()) {
                errors.push((Some("role".into()), STAFF_MANAGED.into()));
            }
            // The platform role is never written on update; a row may only
            // name one the member already holds.
            if platform_role != PlatformRole::User
                && PlatformRole::from_db(&user.platform_role) != platform_role
            {
                errors.push((Some("role".into()), AUDIT_REVIEWER_REFUSED.into()));
            }

            let name_change = (name != user.name).then_some(name);
            if name_change.is_some() {
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
            }
            let pronouns_change =
                opt_string(row, "pronouns").filter(|p| user.pronouns.as_deref() != Some(p));
            if pronouns_change.is_some() && !lookup.may_edit_profile(user) {
                errors.push((
                    Some("pronouns".into()),
                    "only the person or a platform admin can change their pronouns".into(),
                ));
            }

            if !errors.is_empty() {
                return Err(errors);
            }
            Ok(RowAction::UpdateMember {
                user_uuid: user.uuid,
                workspace_role,
                name: name_change,
                pronouns: pronouns_change,
            })
        }
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
