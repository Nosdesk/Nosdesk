//! Row shaping for archive rows: what an export writes for a row and how a
//! load turns an archive row back into one, applying the column policy.
//!
//! Only the policy's columns are shaped. An archive from an earlier schema is
//! migrated before it loads (`backup_upgrade`), never patched here.
//!
//! A load writes only the columns the archive rows carry (plus those a rule
//! sets), so a column a row leaves out takes its default instead of NULL.

use std::collections::HashSet;

use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::Text;

use super::policy::{self, Absent, Secret};
use crate::db::DbConnection;
use crate::services::backup::BackupError;

/// The `SELECT` expression that writes one row of `table` (aliased `t`) as
/// JSON text, leaving out the columns the policy keeps from an unsealed
/// archive. The row stays text end to end so `NUMERIC` precision survives.
pub fn export_row_sql(table: &str, sealed: bool) -> String {
    row_sql_without(&policy::left_out(table, sealed))
}

/// [`export_row_sql`] for a workspace archive, which also leaves out the
/// columns it never carries (`policy::left_out_of_workspace_archive`).
pub fn workspace_export_row_sql(table: &str, sealed: bool) -> String {
    row_sql_without(&policy::left_out_of_workspace_archive(table, sealed))
}

/// The `SELECT` expression that writes one row (aliased `t`) as JSON text
/// without `left_out`.
fn row_sql_without(left_out: &[&str]) -> String {
    if left_out.is_empty() {
        return "row_to_json(t)::text".to_string();
    }
    let mut expr = "row_to_json(t)::jsonb".to_string();
    for column in left_out {
        expr.push_str(&format!(" - {}", quote_literal(column)));
    }
    format!("({expr})::text")
}

/// The `$1` payload (a JSON array of archive rows) as the rows to load: a row
/// a [`Absent::SkipRow`] rule drops is left out and every [`Absent::Set`] rule
/// whose column the row leaves out is applied. All of a table's rules apply.
pub fn payload_rows(secrets: &[Secret]) -> String {
    let mut row = "r".to_string();
    let mut keep: Vec<String> = Vec::new();
    for secret in secrets {
        let column = quote_literal(secret.column);
        match secret.absent {
            Absent::Default => {}
            Absent::SkipRow => keep.push(format!("r ? {column}")),
            Absent::Set(sets) => {
                let fill = sets
                    .iter()
                    .map(|(set, value)| format!("{}, {value}", quote_literal(set)))
                    .collect::<Vec<_>>()
                    .join(", ");
                row = format!(
                    "{row} || CASE WHEN r ? {column} THEN '{{}}'::jsonb \
                     ELSE jsonb_build_object({fill}) END"
                );
            }
        }
    }
    if row == "r" && keep.is_empty() {
        return "$1::jsonb".to_string();
    }
    let keep = if keep.is_empty() {
        "true".to_string()
    } else {
        keep.join(" AND ")
    };
    format!(
        "(SELECT coalesce(jsonb_agg({row}), '[]'::jsonb) \
         FROM jsonb_array_elements($1::jsonb) r WHERE {keep})"
    )
}

/// The keys the rows of a JSON array payload carry.
pub fn payload_keys(
    conn: &mut DbConnection,
    payload: &str,
) -> Result<HashSet<String>, BackupError> {
    #[derive(QueryableByName)]
    struct Key {
        #[diesel(sql_type = Text)]
        key: String,
    }
    let keys: Vec<Key> = sql_query(
        "SELECT DISTINCT k AS key FROM jsonb_array_elements($1::jsonb) e, \
         jsonb_object_keys(e) k WHERE jsonb_typeof(e) = 'object'",
    )
    .bind::<Text, _>(payload)
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    Ok(keys.into_iter().map(|k| k.key).collect())
}

/// The statement that inserts the `$1` payload's rows into `table`: the
/// table's columns that the rows carry (`keys`) or a policy rule sets, minus
/// `omit`, then `suffix` (an `ON CONFLICT` or `RETURNING` clause). `table` must
/// be a real table; column names come from the catalog.
pub fn load_statement(
    conn: &mut DbConnection,
    table: &str,
    keys: &HashSet<String>,
    omit: &[&str],
    suffix: &str,
) -> Result<String, BackupError> {
    if !table
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(BackupError::CorruptedBackup(format!(
            "unsafe table identifier '{table}'"
        )));
    }
    let secrets = policy::secrets(table);
    let set: HashSet<&str> = secrets
        .iter()
        .flat_map(|s| match s.absent {
            Absent::Set(sets) => sets.iter().map(|(column, _)| *column).collect(),
            _ => Vec::new(),
        })
        .collect();
    let columns: Vec<String> = table_columns(conn, table)?
        .into_iter()
        .filter(|c| keys.contains(c) || set.contains(c.as_str()))
        .filter(|c| !omit.contains(&c.as_str()))
        .map(|c| format!("\"{c}\""))
        .collect();
    if columns.is_empty() {
        return Err(BackupError::CorruptedBackup(format!(
            "archive rows for '{table}' carry none of its columns"
        )));
    }
    let columns = columns.join(", ");
    Ok(format!(
        "INSERT INTO public.{table} ({columns}) \
         SELECT {columns} FROM jsonb_populate_recordset(NULL::public.{table}, {rows}) {suffix}",
        rows = payload_rows(secrets),
    ))
}

/// The live columns of `table`, in table order.
fn table_columns(conn: &mut DbConnection, table: &str) -> Result<Vec<String>, BackupError> {
    #[derive(QueryableByName)]
    struct Column {
        #[diesel(sql_type = Text)]
        name: String,
    }
    let columns: Vec<Column> = sql_query(
        "SELECT attname::text AS name FROM pg_attribute \
         WHERE attrelid = to_regclass('public.' || $1) AND attnum > 0 AND NOT attisdropped \
         ORDER BY attnum",
    )
    .bind::<Text, _>(table)
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    Ok(columns.into_iter().map(|c| c.name).collect())
}

/// A SQL string literal. Policy names are code constants; quoting still
/// doubles any single quote.
fn quote_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::workspace_archive::policy::Reentry;
    use crate::test_helpers::setup_test_connection;

    fn shaped(conn: &mut DbConnection, secrets: &[Secret], payload: &str) -> serde_json::Value {
        #[derive(QueryableByName)]
        struct Rows {
            #[diesel(sql_type = Text)]
            rows: String,
        }
        let rows: Rows = sql_query(format!("SELECT ({})::text AS rows", payload_rows(secrets)))
            .bind::<Text, _>(payload)
            .get_result(conn)
            .expect("shape rows");
        serde_json::from_str(&rows.rows).expect("rows json")
    }

    /// Two rules on one table both apply (the first-match lookup this
    /// replaced applied only the first).
    #[test]
    fn every_rule_on_a_table_applies() {
        let mut conn = setup_test_connection();
        let secrets = [
            Secret {
                column: "token",
                absent: Absent::SkipRow,
                reentry: None,
            },
            Secret {
                column: "secret",
                absent: Absent::Set(&[("secret", "'minted'"), ("enabled", "false")]),
                reentry: Some(Reentry::WebhookSecret),
            },
            Secret {
                column: "key",
                absent: Absent::Set(&[("key_id", "NULL")]),
                reentry: None,
            },
        ];
        let rows = shaped(
            &mut conn,
            &secrets,
            r#"[
                {"id": 1, "token": "a", "secret": "s", "key": "k", "key_id": 3, "enabled": true},
                {"id": 2, "enabled": true},
                {"id": 3, "token": "b", "enabled": true, "key_id": 4}
            ]"#,
        );
        assert_eq!(
            rows,
            serde_json::json!([
                {"id": 1, "token": "a", "secret": "s", "key": "k", "key_id": 3, "enabled": true},
                {"id": 3, "token": "b", "enabled": false, "secret": "minted", "key_id": null}
            ])
        );
    }

    #[test]
    fn a_table_without_rules_loads_its_payload_as_is() {
        assert_eq!(payload_rows(&[]), "$1::jsonb");
        let default_only = [Secret {
            column: "headers",
            absent: Absent::Default,
            reentry: None,
        }];
        assert_eq!(payload_rows(&default_only), "$1::jsonb");
    }

    #[test]
    fn an_unsealed_export_leaves_secrets_out() {
        assert_eq!(export_row_sql("tickets", false), "row_to_json(t)::text");
        assert_eq!(export_row_sql("webhooks", true), "row_to_json(t)::text");
        assert_eq!(
            export_row_sql("webhooks", false),
            "(row_to_json(t)::jsonb - 'secret' - 'headers')::text"
        );
    }

    /// A workspace archive never carries sign-in passwords, sealed or not;
    /// an instance backup does when sealed.
    #[test]
    fn a_workspace_archive_never_carries_identity_passwords() {
        assert_eq!(
            export_row_sql("user_auth_identities", true),
            "row_to_json(t)::text"
        );
        for sealed in [true, false] {
            assert_eq!(
                workspace_export_row_sql("user_auth_identities", sealed),
                "(row_to_json(t)::jsonb - 'password_hash' - 'metadata')::text"
            );
        }
        assert_eq!(
            workspace_export_row_sql("webhooks", true),
            "row_to_json(t)::text"
        );
    }

    /// A row that leaves out a NOT NULL column with a default loads with the
    /// default (an archive taken before the column existed, after migration,
    /// or one a tool wrote by hand).
    #[test]
    fn a_row_missing_a_defaulted_column_takes_the_default() {
        let mut conn = setup_test_connection();
        let row = serde_json::json!({
            "uuid": uuid::Uuid::now_v7(),
            "name": "Defaulted",
            "created_at": "2026-01-01T00:00:00",
            "updated_at": "2026-01-01T00:00:00",
        });
        let payload = serde_json::to_string(&[&row]).expect("payload");
        let keys = payload_keys(&mut conn, &payload).expect("keys");
        let stmt = load_statement(
            &mut conn,
            "users",
            &keys,
            &[],
            "RETURNING platform_role::text AS platform_role, \
             feature_flag_overrides::text AS flags, mfa_enabled",
        )
        .expect("statement");
        #[derive(QueryableByName)]
        struct Loaded {
            #[diesel(sql_type = Text)]
            platform_role: String,
            #[diesel(sql_type = Text)]
            flags: String,
            #[diesel(sql_type = diesel::sql_types::Bool)]
            mfa_enabled: bool,
        }
        let loaded: Vec<Loaded> = sql_query(stmt)
            .bind::<Text, _>(payload)
            .load(&mut conn)
            .expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].platform_role, "user");
        assert_eq!(loaded[0].flags, "{}");
        assert!(!loaded[0].mfa_enabled);
    }
}
