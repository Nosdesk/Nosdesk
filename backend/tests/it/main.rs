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

mod admin_workspace_members;
mod analytics_kpi_summary;
mod asset_audits;
mod asset_kinds_picker;
mod audit_context_lint;
mod audit_redaction;
mod audit_unified;
mod background_run_cross_tenant_lint;
mod background_workspace_pin;
mod backup_basic;
mod backup_encryption;
mod backup_q_smoke;
mod backup_sequences;
mod backup_tamper;
mod bearer_auth;
mod collab_fenced_write;
mod collab_ownership;
mod cross_tenant_workspace_lookup;
mod csrf_origin_check;
mod directory_scoping;
mod documentation_export_acl;
mod dsn_corpus;
mod email_quote_corpus;
mod email_suppression_scoping;
mod email_verification_round_trip;
mod guest_pending_hold;
mod handler_direct_write_lint;
mod handler_error_builder_lint;
mod handler_tenant_read_lint;
mod idempotency_middleware;
mod imports_assets;
mod imports_tickets;
mod imports_users;
mod integration_routes_require_admin;
mod knowledge_gap_lifecycle;
mod logging_pii_guardrail;
mod migration_backfill_existing_workspace;
mod notification_deliveries;
mod notification_inbox_filters;
mod notification_outbox;
mod notification_workspace_scope;
mod plugin_bundle_isolation;
mod plugin_collection_row_scoping;
mod plugin_permission_gate;
mod portal_session;
mod profile_write_authz;
mod projection_membership_role_model;
mod push_preference_defaults;
mod route_auth_funnel_lint;
mod site_settings_per_workspace;
mod sync_emit_lint;
mod sync_model_registry;
mod sync_ticket_group_auth;
mod sync_visibility;
mod tenant_table_grants_lint;
mod tenant_table_rls_lint;
mod ticket_activity_visibility;
mod ticket_merge;
mod tracing_field_allowlist_lint;
mod two_workspace_fixture;
mod user_contact_gate;
mod webhook_outbox_durability;
mod workspace_fk_cascade_lint;
mod workspace_member_soft_remove;
mod workspace_members_active_filter_lint;
mod workspace_membership_gate;
mod workspace_portal;
mod workspace_role_resolution;
mod workspace_smtp_relay;
