//! One test binary for every integration test that leaves process-global
//! state alone. Each `tests/*.rs` file is its own binary that links the whole
//! backend, so 84 of them spent most of a CI run linking; this crate links
//! once.
//!
//! A test stays a standalone file in `tests/` when it cannot share a process
//! with the rest, since tests here run in parallel threads of one process:
//! it sets environment variables or cached process-wide state (deployment
//! mode, the licence; `common::enable_platform_auth` flips the process to
//! hosted), or it asserts on something cluster-wide in Postgres
//! (`sync_commit_cursor` reads the commit horizon, which any concurrent
//! transaction moves).

#![allow(clippy::expect_used)]

#[path = "../common/mod.rs"]
mod common;

mod add_requester;
mod admin_workspace_members;
mod analytics_breakdown_labels;
mod analytics_kpi_summary;
mod api_token_role_ceiling;
mod asset_audits;
mod asset_kinds_picker;
mod assignee_eligibility;
mod assignment_on_create;
mod attachment_claims;
mod attachment_emit_lint;
mod audit_context_lint;
mod audit_redaction;
mod audit_unified;
mod background_run_cross_tenant_lint;
mod background_workspace_pin;
mod backup_basic;
mod backup_encryption;
mod backup_plaintext;
mod backup_q_smoke;
mod backup_sequences;
mod backup_tamper;
mod bearer_auth;
mod closed_follows_state;
mod collab_fenced_write;
mod collab_ownership;
mod cross_layer_lists;
mod cross_tenant_workspace_lookup;
mod csrf_origin_check;
mod directory_scoping;
mod directory_sync_workspace_scope;
mod documentation_export_acl;
mod documentation_hidden_pages;
mod dsn_corpus;
mod email_locale_per_workspace;
mod email_logo_copies;
mod email_quote_corpus;
mod email_suppression_scoping;
mod email_verification_round_trip;
mod foreign_references_refused;
mod guest_pending_hold;
mod guest_residue_sweep;
mod handler_direct_write_lint;
mod handler_error_builder_lint;
mod handler_tenant_read_lint;
mod idempotency_middleware;
mod imports_assets;
mod imports_tickets;
mod imports_users;
mod integration_routes_require_admin;
mod knowledge_gap_hidden_pages;
mod knowledge_gap_lifecycle;
mod ldap_groups_in_workspace;
mod logging_pii_guardrail;
mod merged_ticket_writes;
mod migration_api_token_role_ceiling;
mod migration_backfill_existing_workspace;
mod migration_email_suppressions_scope;
mod migration_knowledge_gap_subject_page;
mod migration_merge_notes_internal;
mod migration_reserved_slugs;
mod migration_ticket_numbers;
mod migration_version_lint;
mod migration_workspace_scoped_keys;
mod notification_deliveries;
mod notification_digest;
mod notification_inbox_filters;
mod notification_outbox;
mod notification_workspace_scope;
mod partition_prune;
mod people_removed_under_1_0;
mod plugin_bundle_isolation;
mod plugin_collection_row_scoping;
mod plugin_events_as_events;
mod plugin_permission_gate;
mod portal_merged_tickets;
mod portal_session;
mod priority_names;
mod profile_write_authz;
mod project_ticket_visibility;
mod projection_membership_role_model;
mod push_preference_defaults;
mod rebuild_search_index;
mod recurring_tickets;
mod reply_sent_twice;
mod route_auth_funnel_lint;
mod rules_in_workspace;
mod scheduler_status;
mod search_by_ticket_number;
mod site_settings_per_workspace;
mod sla_breach_sweep;
mod sla_defaults;
mod sla_policy_priority;
mod sync_audiences;
mod sync_emit_lint;
mod sync_model_registry;
mod sync_ticket_group_auth;
mod sync_visibility;
mod sync_workspace_people;
mod tenant_table_grants_lint;
mod tenant_table_rls_lint;
mod test_db_template;
mod ticket_activity_visibility;
mod ticket_by_number;
mod ticket_create_columns;
mod ticket_file_access;
mod ticket_join_write_lint;
mod ticket_merge;
mod ticket_numbers;
mod ticket_update_routes;
mod ticket_url_lint;
mod ticket_write_lint;
mod tracing_field_allowlist_lint;
mod two_workspace_fixture;
mod upgrade_from_1_0_12;
mod user_contact_gate;
mod webhook_delivery_in_workspace;
mod webhook_outbox_durability;
mod workflow_state_restore;
mod workflow_states_per_workspace;
mod workspace_branding;
mod workspace_file_purge;
mod workspace_fk_cascade_lint;
mod workspace_fk_scope_lint;
mod workspace_member_soft_remove;
mod workspace_members_active_filter_lint;
mod workspace_membership_gate;
mod workspace_portal;
mod workspace_role_resolution;
mod workspace_scoped_keys;
mod workspace_smtp_relay;
