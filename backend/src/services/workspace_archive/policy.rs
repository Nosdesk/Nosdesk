//! How each table moves between workspaces, and which of its columns an
//! archive may leave out.
//!
//! Every workspace-scoped table has one entry in [`WORKSPACE_TABLES`]; a test
//! fails when a migration adds one without a class, or when an entry names a
//! table or column the schema no longer has. Instance tables that hold secrets
//! (`users` and the session and recovery tables) are listed in
//! [`INSTANCE_TABLES`] for their columns only.
//!
//! Readers: the instance backup and the workspace export leave out
//! [`Table::secrets`] when unsealed; instance restore and the workspace import
//! load those rows through `rows::load_statement`, which applies each secret's
//! [`Absent`] rule; the workspace export and import carry only tables whose
//! [`Class::carried`] is true.

/// How a workspace-scoped table moves from one workspace into another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Workspace content. The archive carries it and the import adds its rows,
    /// remapped; a target that already holds any is not a fresh workspace.
    Move,
    /// Configuration a new workspace is seeded with. The archive carries it;
    /// the import deletes the target's rows and loads the archive's.
    Replace,
    /// Rows that belong to people. The archive carries it; the target's rows
    /// stay and the archive's join them.
    Merge,
    /// Never carried: credentials and capabilities, delivery queues and logs,
    /// import history, and state the target rebuilds. The target's rows stay.
    Never,
    /// Not workspace content: the audit trail, the sync stream and export jobs.
    Excluded,
}

impl Class {
    /// Whether a workspace archive carries the table.
    pub fn carried(self) -> bool {
        matches!(self, Class::Move | Class::Replace | Class::Merge)
    }
}

/// What a load does with an archive row that leaves a secret column out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Absent {
    /// The column takes its default.
    Default,
    /// The row isn't loaded: it is nothing without the column.
    SkipRow,
    /// The row loads with these columns set, each value SQL (it may read the
    /// archive row as `r`). Fills a NOT NULL column or turns off what the
    /// secret switched on.
    Set(&'static [(&'static str, &'static str)]),
}

/// What someone does again after a load that left a secret out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reentry {
    /// Give the webhook receiver the new signing secret and turn it back on.
    WebhookSecret,
    /// Enter the webhook's custom headers again.
    WebhookHeaders,
    /// Create new API tokens.
    ApiTokens,
    /// Staff set up MFA again.
    Mfa,
    /// People generate new recovery codes.
    RecoveryCodes,
    /// People set a password through a reset link.
    Password,
}

/// A column holding authentication material. A sealed archive carries it; a
/// plaintext one (instance backup or workspace export) leaves it out.
#[derive(Debug, Clone, Copy)]
pub struct Secret {
    pub column: &'static str,
    pub absent: Absent,
    pub reentry: Option<Reentry>,
}

/// One workspace-scoped table's policy.
#[derive(Debug)]
pub struct Table {
    pub name: &'static str,
    pub class: Class,
    pub secrets: &'static [Secret],
}

/// An instance (not workspace-scoped) table with secret columns.
#[derive(Debug)]
pub struct InstanceTable {
    pub name: &'static str,
    pub secrets: &'static [Secret],
}

const fn t(name: &'static str, class: Class) -> Table {
    Table {
        name,
        class,
        secrets: &[],
    }
}

use Class::{Excluded, Merge, Move, Never, Replace};

/// Every workspace-scoped table (a live `workspace_id` column), by name.
pub const WORKSPACE_TABLES: &[Table] = &[
    // Tickets and what hangs off them.
    t("tickets", Move),
    t("comments", Move),
    t("attachments", Move),
    t("comment_ticket_references", Move),
    t("linked_tickets", Move),
    t("ticket_merges", Move),
    t("ticket_tags", Move),
    t("ticket_watchers", Move),
    t("ticket_assets", Move),
    t("ticket_approvals", Move),
    t("ticket_ratings", Move),
    t("user_ticket_views", Move),
    t("article_contents", Move),
    t("article_content_revisions", Move),
    t("tags", Move),
    t("canned_responses", Move),
    t("canned_response_insertions", Move),
    t("assignment_rules", Move),
    t("assignment_rule_state", Move),
    t("assignment_log", Move),
    t("workspace_notices", Move),
    // Projects and cycles.
    t("projects", Move),
    t("project_tickets", Move),
    t("cycles", Move),
    t("cycle_tickets", Move),
    // Assets.
    t("assets", Move),
    t("asset_audits", Move),
    t("asset_directory_memberships", Move),
    t("asset_disposals", Move),
    t("asset_group_assignments", Move),
    t("asset_groups", Move),
    t("asset_lifecycle_events", Move),
    t("asset_loans", Move),
    t("asset_media", Move),
    t("asset_models", Move),
    t("asset_usage_log", Move),
    t("manufacturers", Move),
    // Groups.
    t("groups", Move),
    t("group_includes", Move),
    t("user_groups", Move),
    // Documentation beyond the seeded Getting Started collection.
    t("documentation_collection_visibility", Move),
    t("documentation_page_visibility", Move),
    t("documentation_page_tickets", Move),
    t("documentation_page_embeddings", Move),
    t("documentation_starred_pages", Move),
    t("documentation_subscriptions", Move),
    // Knowledge gaps.
    t("knowledge_gaps", Move),
    t("knowledge_gap_signals", Move),
    // Rules and views.
    t("rules", Move),
    t("rule_versions", Move),
    t("rule_applications", Move),
    t("saved_views", Move),
    // Integrations. Their sealed credentials stay bound to this instance's
    // keyring and ids (encrypted_* columns), so they don't decrypt elsewhere.
    t("channels", Move),
    t("channel_messages", Move),
    t("outbound_emails", Move),
    Table {
        name: "webhooks",
        class: Move,
        secrets: &[
            // A webhook gets a new secret, and stays off until an admin gives
            // the receiver the new one.
            Secret {
                column: "secret",
                absent: Absent::Set(&[
                    (
                        "secret",
                        "'whsec_' || encode(public.gen_random_bytes(32), 'hex')",
                    ),
                    ("enabled", "false"),
                ]),
                reentry: Some(Reentry::WebhookSecret),
            },
            // Custom headers commonly carry an Authorization value for the
            // receiver.
            Secret {
                column: "headers",
                absent: Absent::Default,
                reentry: Some(Reentry::WebhookHeaders),
            },
        ],
    },
    t("plugins", Move),
    t("plugin_data", Move),
    t("plugin_activity", Move),
    t("plugin_collection_schemas", Move),
    t("plugin_collection_rows", Move),
    t("workspace_email_settings", Move),
    t("workspace_identity_providers", Move),
    t("workspace_ldap_settings", Move),
    t("workspace_widget_settings", Move),
    // Seeded configuration.
    t("workflow_states", Replace),
    t("sla_policies", Replace),
    t("working_calendars", Replace),
    t("working_calendar_holidays", Replace),
    t("ticket_categories", Replace),
    t("category_approvers", Replace),
    t("category_group_visibility", Replace),
    t("asset_kinds", Replace),
    t("site_settings", Replace),
    t("user_field_schema", Replace),
    t("workspace_notification_defaults", Replace),
    t("documentation_collections", Replace),
    t("documentation_collection_pages", Replace),
    t("documentation_pages", Replace),
    t("documentation_revisions", Replace),
    // People's rows.
    t("workspace_members", Merge),
    t("user_profiles", Merge),
    t("user_addresses", Merge),
    t("user_phone_numbers", Merge),
    Table {
        name: "user_auth_identities",
        class: Merge,
        secrets: &[
            Secret {
                column: "password_hash",
                absent: Absent::Default,
                reentry: Some(Reentry::Password),
            },
            Secret {
                column: "metadata",
                absent: Absent::Default,
                reentry: None,
            },
        ],
    },
    t("notification_preferences", Merge),
    t("notifications", Merge),
    t("email_suppressions", Merge),
    t("search_query_log", Merge),
    // Never carried.
    Table {
        name: "api_tokens",
        class: Never,
        // SHA-256 of a 128-bit random token: low impact, but kept out of
        // plaintext backups like refresh tokens are.
        secrets: &[Secret {
            column: "token_hash",
            absent: Absent::SkipRow,
            reentry: Some(Reentry::ApiTokens),
        }],
    },
    t("channel_credentials", Never),
    t("inbound_addresses", Never),
    t("user_push_devices", Never),
    t("webhook_deliveries", Never),
    t("notification_deliveries", Never),
    t("sync_delta_tokens", Never),
    t("sync_history", Never),
    t("workspace_ldap_sync_state", Never),
    t("yjs_snapshots", Never),
    t("import_jobs", Never),
    t("import_value_answers", Never),
    t("backup_jobs", Never),
    t("bug_reports", Never),
    t("csp_reports", Never),
    // Not workspace content.
    t("audit_log", Excluded),
    t("sync_actions", Excluded),
    t("workspace_export_jobs", Excluded),
];

/// Instance tables with secret columns. Instance backups carry these tables;
/// a workspace archive carries only the `users` its rows name.
pub const INSTANCE_TABLES: &[InstanceTable] = &[
    // No MFA secret means no MFA (and no key id for it): staff are asked to
    // set it up again.
    InstanceTable {
        name: "users",
        secrets: &[Secret {
            column: "mfa_secret",
            absent: Absent::Set(&[("mfa_enabled", "false"), ("mfa_secret_kek_id", "NULL")]),
            reentry: Some(Reentry::Mfa),
        }],
    },
    // Tokens and recovery codes are nothing without their hashes.
    InstanceTable {
        name: "user_recovery_codes",
        secrets: &[Secret {
            column: "code_hash",
            absent: Absent::SkipRow,
            reentry: Some(Reentry::RecoveryCodes),
        }],
    },
    InstanceTable {
        name: "refresh_tokens",
        secrets: &[Secret {
            column: "token_hash",
            absent: Absent::SkipRow,
            reentry: None,
        }],
    },
    InstanceTable {
        name: "reset_tokens",
        secrets: &[
            Secret {
                column: "token_hash",
                absent: Absent::SkipRow,
                reentry: None,
            },
            Secret {
                column: "metadata",
                absent: Absent::Default,
                reentry: None,
            },
        ],
    },
];

/// The workspace table's policy, if it has one.
pub fn table(name: &str) -> Option<&'static Table> {
    WORKSPACE_TABLES.iter().find(|t| t.name == name)
}

/// The secret columns of any table.
pub fn secrets(name: &str) -> &'static [Secret] {
    table(name).map(|t| t.secrets).unwrap_or_else(|| {
        INSTANCE_TABLES
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.secrets)
            .unwrap_or(&[])
    })
}

/// The columns an archive of `name` leaves out: its secrets, unless sealed.
pub fn left_out(name: &str, sealed: bool) -> Vec<&'static str> {
    if sealed {
        return Vec::new();
    }
    secrets(name).iter().map(|s| s.column).collect()
}

/// Whether a workspace archive carries the workspace table `name`. A table
/// the policy doesn't know is carried, as every new table was before the
/// policy; the completeness test keeps that from shipping.
pub fn carried(name: &str) -> bool {
    table(name).is_none_or(|t| t.class.carried())
}

/// Whether the import skips `name` in an archive rather than refusing it: a
/// table the policy keeps out of archives, which an older one may carry.
pub fn skipped_on_import(name: &str) -> bool {
    table(name).is_some_and(|t| !t.class.carried())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;
    use diesel::prelude::*;
    use diesel::sql_query;
    use diesel::sql_types::Text;
    use std::collections::{BTreeSet, HashSet};

    #[derive(QueryableByName)]
    struct Name {
        #[diesel(sql_type = Text)]
        name: String,
    }

    fn columns(conn: &mut crate::db::DbConnection, table: &str) -> HashSet<String> {
        sql_query(
            "SELECT attname::text AS name FROM pg_attribute \
             WHERE attrelid = to_regclass('public.' || $1) AND attnum > 0 AND NOT attisdropped",
        )
        .bind::<Text, _>(table)
        .load::<Name>(conn)
        .expect("columns")
        .into_iter()
        .map(|n| n.name)
        .collect()
    }

    /// A migration that adds a workspace table fails here until the table is
    /// classified, and a dropped table's entry has to go.
    #[test]
    fn every_workspace_table_has_a_policy() {
        let mut conn = setup_test_connection();
        let discovered: BTreeSet<String> =
            crate::services::workspace_export::all_workspace_tables(&mut conn)
                .expect("discover")
                .into_iter()
                .collect();
        let listed: BTreeSet<String> = WORKSPACE_TABLES
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        assert_eq!(
            listed.len(),
            WORKSPACE_TABLES.len(),
            "a table is listed twice"
        );
        let unclassified: Vec<_> = discovered.difference(&listed).collect();
        assert!(
            unclassified.is_empty(),
            "workspace tables without a class in workspace_archive::policy: {unclassified:?}"
        );
        let stale: Vec<_> = listed.difference(&discovered).collect();
        assert!(
            stale.is_empty(),
            "policy entries for tables that aren't workspace-scoped: {stale:?}"
        );
        for instance in INSTANCE_TABLES {
            assert!(
                !discovered.contains(instance.name)
                    && !columns(&mut conn, instance.name).is_empty(),
                "{} is an instance table",
                instance.name
            );
        }
    }

    /// A migration that drops or renames a column the policy names fails here.
    #[test]
    fn every_column_a_policy_names_exists() {
        let mut conn = setup_test_connection();
        let named = WORKSPACE_TABLES
            .iter()
            .map(|t| (t.name, t.secrets))
            .chain(INSTANCE_TABLES.iter().map(|t| (t.name, t.secrets)));
        for (table, secrets) in named {
            let have = columns(&mut conn, table);
            for secret in secrets {
                let mut named = vec![secret.column];
                if let Absent::Set(sets) = secret.absent {
                    named.extend(sets.iter().map(|(column, _)| *column));
                }
                for column in named {
                    assert!(
                        have.contains(column),
                        "{table}.{column} isn't in the schema"
                    );
                }
            }
        }
    }

    /// The import keeps a foreign key into a table it didn't load pointing at
    /// the source's id, so a carried table may only reference carried ones.
    #[test]
    fn carried_tables_reference_only_carried_tables() {
        #[derive(QueryableByName)]
        struct Edge {
            #[diesel(sql_type = Text)]
            child: String,
            #[diesel(sql_type = Text)]
            parent: String,
        }
        let mut conn = setup_test_connection();
        let edges: Vec<Edge> = sql_query(
            "SELECT conrelid::regclass::text AS child, confrelid::regclass::text AS parent \
             FROM pg_constraint WHERE contype = 'f' AND connamespace = 'public'::regnamespace",
        )
        .load(&mut conn)
        .expect("foreign keys");
        for edge in edges {
            let (Some(child), Some(parent)) = (table(&edge.child), table(&edge.parent)) else {
                continue;
            };
            assert!(
                !child.class.carried() || parent.class.carried(),
                "{} is carried but references {}, which isn't",
                edge.child,
                edge.parent
            );
        }
    }
}
