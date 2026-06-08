//! User CSV importer.
//!
//! Natural key: `email` (looked up against `user_emails`). Rows
//! whose email matches a primary email upsert the user row;
//! rows with a new email INSERT a fresh user plus a primary
//! `user_emails` entry. Auth identities are out of scope for
//! the import path. Bulk-imported users get a passwordless row
//! that admins later associate with an identity provider, send
//! a reset link to, or invite to set up MFA.
//!
//! Columns:
//!   - email   (required, the natural key)
//!   - name    (required)
//!   - role    (required: admin | technician | user)
//!   - pronouns (optional)

use std::collections::{HashMap, HashSet};

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewUser, UserUpdate};
use crate::repository::user_helpers::create_user_with_email;
use crate::repository::users::update_user;

use super::csv_parser::ParsedCsv;
use super::types::{ImportSummary, ImportedRecords, ImportedUser, Importer, RowError, MAX_ERRORS};

const HEADERS: &[&str] = &["email", "name", "role", "pronouns"];

pub struct UserImporter;

impl Importer for UserImporter {
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

        let existing_emails = load_existing_primary_emails(conn)?;
        let mut emails_in_file: HashSet<String> = HashSet::new();

        for (i, row) in parsed.rows.iter().enumerate() {
            let row_num = i + 2;
            match validate_row(row, &existing_emails, &mut emails_in_file) {
                Ok(RowAction::Create) => summary.would_create += 1,
                Ok(RowAction::Update(_)) => summary.would_update += 1,
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
        use crate::schema::users;
        if check_headers(&parsed.headers).is_some() {
            return Err(diesel::result::Error::QueryBuilderError(
                "header validation should have caught this; refusing to commit".into(),
            ));
        }
        let existing_emails = load_existing_primary_emails(conn)?;
        let mut emails_in_file: HashSet<String> = HashSet::new();

        let mut imported: Vec<ImportedUser> = Vec::new();
        for row in &parsed.rows {
            let mut local = emails_in_file.clone();
            let action = match validate_row(row, &existing_emails, &mut local) {
                Ok(a) => a,
                Err(_) => continue,
            };
            emails_in_file = local;

            let name = trimmed(row, "name");
            // Map the CSV "role" column onto the W2 split
            // (platform_role + workspace_members.role) via the shared
            // parser, so import and the create-user API agree on what
            // each role string means.
            let (platform_role_enum, workspace_role_enum) =
                crate::utils::parse_roles(trimmed(row, "role").as_str()).expect("validated above");
            let pronouns = opt_string(row, "pronouns");
            let email = trimmed(row, "email");
            match action {
                RowAction::Create => {
                    let new_user = NewUser {
                        uuid: Uuid::new_v4(),
                        name,
                        pronouns,
                        avatar_url: None,
                        banner_url: None,
                        avatar_thumb: None,
                        microsoft_uuid: None,
                        mfa_secret: None,
                        mfa_secret_kek_id: None,
                        mfa_enabled: false,
                        platform_role: Some(platform_role_enum.as_str().to_string()),
                    };
                    // create_user_with_email writes the user, the primary
                    // email, and the workspace_members row (workspace from
                    // app.workspace_id, pinned by TenantConn) and emits
                    // user.created, the same path the create-user API uses.
                    // observer = None: the import handler indexes every
                    // committed entity post-commit, so all three importers
                    // index uniformly without per-importer observer wiring.
                    let (user, user_email) = create_user_with_email(
                        new_user,
                        workspace_role_enum,
                        email,
                        false, // email starts unverified; admin / IdP verifies later
                        Some("csv_import".to_string()),
                        conn,
                        None,
                    )?;
                    imported.push(ImportedUser {
                        user,
                        primary_email: Some(user_email.email),
                    });
                }
                RowAction::Update(uuid) => {
                    // Role changes are operator-side and intentionally not
                    // sync-emitted (mirrors the admin bulk-role handler and
                    // the workspace_members "sync-audit-only" rule): raw-write
                    // the platform_role and the membership role.
                    diesel::update(users::table.find(uuid))
                        .set((
                            users::platform_role.eq(platform_role_enum.as_str()),
                            users::updated_at.eq(chrono::Utc::now().naive_utc()),
                        ))
                        .execute(conn)?;
                    diesel::sql_query(
                        "UPDATE workspace_members \
                         SET role = $2 \
                         WHERE workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int \
                           AND user_uuid = $1",
                    )
                    .bind::<diesel::sql_types::Uuid, _>(uuid)
                    .bind::<diesel::sql_types::Text, _>(workspace_role_enum.as_str())
                    .execute(conn)?;
                    // The profile-field change goes through update_user, which
                    // emits user.updated and returns the refreshed model for
                    // indexing.
                    let user = update_user(
                        &uuid,
                        UserUpdate {
                            name: Some(name),
                            pronouns,
                            avatar_url: None,
                            banner_url: None,
                            avatar_thumb: None,
                            microsoft_uuid: None,
                            updated_at: None,
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
    Create,
    Update(Uuid),
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

fn load_existing_primary_emails(
    conn: &mut DbConnection,
) -> Result<HashMap<String, Uuid>, diesel::result::Error> {
    use crate::schema::user_emails;
    let rows: Vec<(String, Uuid)> = user_emails::table
        .filter(user_emails::is_primary.eq(true))
        .select((user_emails::email, user_emails::user_uuid))
        .load(conn)?;
    // Lower-case the key for case-insensitive matching at upsert
    // time; the original casing stays in the DB.
    Ok(rows
        .into_iter()
        .map(|(e, u)| (e.to_lowercase(), u))
        .collect())
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
    existing_emails: &HashMap<String, Uuid>,
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
    if role_raw.is_empty() {
        errors.push((Some("role".into()), "role is required".into()));
    } else if crate::utils::parse_roles(&role_raw).is_err() {
        errors.push((
            Some("role".into()),
            format!("'{role_raw}' is not a valid role; use admin, technician, or user"),
        ));
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    let email_key = email.to_lowercase();
    if !emails_in_file.insert(email_key.clone()) {
        return Err(vec![(
            Some("email".into()),
            format!("email '{email}' appears more than once in this file"),
        )]);
    }

    let action = match existing_emails.get(&email_key) {
        Some(uuid) => RowAction::Update(*uuid),
        None => RowAction::Create,
    };
    Ok(action)
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
