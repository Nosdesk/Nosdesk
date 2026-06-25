//! Nightly LDAP full reconcile (the scheduler job body). Lives next to the sync
//! engine rather than in the HTTP handler.
//!
//! It resets the DirSync cursor and runs a FULL sync for the LDAP-enabled
//! bootstrap workspace, catching any drift the incremental DirSync stream missed
//! and priming the complete current-directory set for the future deprovision
//! pass. No-op when LDAP isn't configured.
//!
//! Self-hosted is the only LDAP consumer and is single-workspace, so this
//! targets `BOOTSTRAP_WORKSPACE_ID` like the scheduled Microsoft Graph sync; a
//! multi-workspace scan would only matter for a cloud-LDAP path that doesn't
//! exist (cloud directory sync is SCIM).

use anyhow::{Context, Result};
use tracing::{info, warn};

use crate::db::{DbConnection, Pool};
use crate::repository::workspace_ldap_settings;
use crate::services::ldap::sync;
use crate::sync::actor::{ActorContext, BOOTSTRAP_WORKSPACE_ID};
use crate::sync::session::{elevate_session_role, reset_session_role};

/// Run the nightly reconcile. Sets up the background workspace context (no
/// request), runs the reconcile, and resets the connection's role/GUCs before
/// returning it to the pool, mirroring `run_scheduled_delta_sync`.
pub async fn run_scheduled_reconcile(pool: &Pool) -> Result<()> {
    let workspace_id = BOOTSTRAP_WORKSPACE_ID;
    let mut conn = pool.get().context("ldap reconcile: db conn")?;

    let actor = ActorContext::system("scheduler:ldap_reconcile").with_workspace(workspace_id);
    elevate_session_role(&mut conn, &actor).context("ldap reconcile: elevate session")?;

    let outcome = reconcile_workspace(&mut conn, workspace_id).await;

    // Reset before the pooled connection is reused, so the bypass role + the
    // pinned workspace GUC can't leak across checkouts.
    reset_session_role(&mut conn);
    outcome
}

async fn reconcile_workspace(conn: &mut DbConnection, workspace_id: i32) -> Result<()> {
    let settings = match workspace_ldap_settings::get_for_workspace(conn, workspace_id)
        .context("load ldap settings")?
    {
        Some(s) if s.enabled => s,
        // Not configured or disabled: nothing to reconcile.
        _ => return Ok(()),
    };
    let bind_password = workspace_ldap_settings::decrypt_bind_password(&settings)
        .context("decrypt bind password")?
        .unwrap_or_default();

    // Reset the cursor so this run is a FULL DirSync snapshot (an empty cookie
    // makes AD return everything), catching drift the incremental stream missed.
    // sync_users re-stores a fresh cookie on completion.
    workspace_ldap_settings::set_cookie(conn, workspace_id, "dirsync", None)
        .context("reset dirsync cursor")?;

    match sync::run_recorded_sync(
        conn,
        &settings,
        workspace_id,
        &bind_password,
        "ldap_reconcile",
    )
    .await
    {
        Ok(rec) => {
            info!(
                workspace_id,
                seen = rec.stats.seen,
                synced = rec.stats.synced,
                errors = rec.stats.errors,
                "scheduler: ldap nightly reconcile completed"
            );
            Ok(())
        }
        Err(e) => {
            warn!(workspace_id, error = %e, "scheduler: ldap nightly reconcile failed");
            Err(anyhow::anyhow!("ldap reconcile: {e}"))
        }
    }
}
