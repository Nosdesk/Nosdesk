//! One test binary for the integration tests that run in hosted mode.
//!
//! The deployment mode is process-wide environment, and
//! `DeploymentMode::current()` caches it on first read, so these can't share
//! `tests/it`, which runs self-hosted. They need the same hosted environment,
//! so they share one process instead of linking a binary each. Every test
//! calls [`hosted_env`] first, before anything reads the mode.
//!
//! A hosted test that also flips the process back to self-hosted, or clears
//! the selection flag, can't run here beside the others: it stays a standalone
//! file in `tests/`.

#![allow(clippy::expect_used)]

#[path = "../common/mod.rs"]
mod common;

mod hosted_admin_workspaces_gate;
mod hosted_docs_export;
mod hosted_file_access;
mod hosted_guest_confirm;
mod hosted_person_settings;
mod hosted_portal_requester;
mod hosted_scheduler_status;
mod hosted_signout_others;
mod hosted_workspace_app_name;
mod hosted_workspace_billing;
mod hosted_workspace_import_gate;
mod imports_users_hosted;
mod internal_workspace_activation;
mod internal_workspace_provisioning;
mod internal_workspaces_create;
mod internal_workspaces_custom_domain;
mod internal_workspaces_deprovision;
mod internal_workspaces_set_member_role;
mod internal_workspaces_upsert_projected_user;
mod merge_notice_hosted;
mod workspace_cap_hosted;

/// Put the process in hosted mode, as a hosted deployment runs: the agent app
/// names its workspace in the selection header, and tenant and inbound mail
/// addresses hang off their domains. Set once; nothing here clears them.
///
/// `Once` blocks every other caller until the first has finished, so no test
/// reads the environment before it is set, provided each test calls this
/// before anything else.
pub fn hosted_env() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
        std::env::set_var("NOSDESK_WORKSPACE_SELECTION", "1");
        std::env::set_var("NOSDESK_TENANT_DOMAIN", "nosdesk.test");
        std::env::set_var("NOSDESK_INBOUND_DOMAIN", "inbound.nosdesk.test");
    });
}
