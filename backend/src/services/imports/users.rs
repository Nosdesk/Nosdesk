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
use crate::models::{NewUser, NewUserEmail, UserRole};

use super::csv_parser::ParsedCsv;
use super::types::{ImportSummary, Importer, RowError, MAX_ERRORS};

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
    ) -> Result<i32, diesel::result::Error> {
        use crate::schema::{user_emails, users};
        if check_headers(&parsed.headers).is_some() {
            return Err(diesel::result::Error::QueryBuilderError(
                "header validation should have caught this; refusing to commit".into(),
            ));
        }
        let existing_emails = load_existing_primary_emails(conn)?;
        let mut emails_in_file: HashSet<String> = HashSet::new();

        let mut committed = 0i32;
        for row in &parsed.rows {
            let mut local = emails_in_file.clone();
            let action = match validate_row(row, &existing_emails, &mut local) {
                Ok(a) => a,
                Err(_) => continue,
            };
            emails_in_file = local;

            let name = trimmed(row, "name");
            let role = parse_role(trimmed(row, "role").as_str()).expect("validated above");
            let pronouns = opt_string(row, "pronouns");

            // Map the legacy import "role" column onto the W2
            // split: admin/technician/user → platform_role +
            // workspace_members.role. Without this, AuthContext
            // derives `User` for every imported user because the
            // new sources of truth are empty.
            let platform_role = match role {
                UserRole::Admin => Some("platform_admin".to_string()),
                _ => None,
            };
            let workspace_role = match role {
                UserRole::Admin => "admin",
                UserRole::Technician => "agent",
                _ => "member",
            };
            match action {
                RowAction::Create => {
                    let new_uuid = Uuid::new_v4();
                    // `role` parsed from the CSV row drives platform_role
                    // + workspace_members.role; the legacy column is gone.
                    let _ = role;
                    diesel::insert_into(users::table)
                        .values(&NewUser {
                            uuid: new_uuid,
                            name: name.clone(),
                            pronouns: pronouns.clone(),
                            avatar_url: None,
                            banner_url: None,
                            avatar_thumb: None,
                            microsoft_uuid: None,
                            mfa_secret: None,
                            mfa_secret_kek_id: None,
                            mfa_enabled: false,
                            platform_role,
                        })
                        .execute(conn)?;
                    diesel::insert_into(user_emails::table)
                        .values(&NewUserEmail {
                            user_uuid: new_uuid,
                            email: trimmed(row, "email"),
                            email_type: "primary".to_string(),
                            is_primary: true,
                            is_verified: false,
                            source: Some("csv_import".to_string()),
                        })
                        .execute(conn)?;
                    // Workspace membership in the request's workspace.
                    // The import commit runs under TenantConn, which
                    // pins `app.workspace_id`, so the membership lands
                    // in the importing user's workspace under hosted
                    // multi-tenancy (the bootstrap workspace in
                    // single-tenant).
                    diesel::sql_query(
                        "INSERT INTO workspace_members (workspace_id, user_uuid, role) \
                         VALUES (NULLIF(current_setting('app.workspace_id', true), '')::int, $1, $2) \
                         ON CONFLICT (workspace_id, user_uuid) DO NOTHING",
                    )
                    .bind::<diesel::sql_types::Uuid, _>(new_uuid)
                    .bind::<diesel::sql_types::Text, _>(workspace_role)
                    .execute(conn)?;
                    committed += 1;
                }
                RowAction::Update(uuid) => {
                    diesel::update(users::table.find(uuid))
                        .set((
                            users::name.eq(name),
                            users::pronouns.eq(pronouns),
                            users::platform_role
                                .eq(platform_role.clone().unwrap_or_else(|| "user".to_string())),
                        ))
                        .execute(conn)?;
                    // Bump the workspace role too so the post-W2
                    // derivation tracks the imported intent.
                    diesel::sql_query(
                        "UPDATE workspace_members \
                         SET role = $2 \
                         WHERE workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int \
                           AND user_uuid = $1",
                    )
                    .bind::<diesel::sql_types::Uuid, _>(uuid)
                    .bind::<diesel::sql_types::Text, _>(workspace_role)
                    .execute(conn)?;
                    committed += 1;
                }
            }
        }
        Ok(committed)
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
    } else if parse_role(&role_raw).is_none() {
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

fn parse_role(s: &str) -> Option<UserRole> {
    match s.to_lowercase().as_str() {
        "admin" => Some(UserRole::Admin),
        "technician" => Some(UserRole::Technician),
        "user" => Some(UserRole::User),
        "audit_reviewer" => Some(UserRole::AuditReviewer),
        _ => None,
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
