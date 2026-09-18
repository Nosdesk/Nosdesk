//! Activation state of one workspace: the facts that say whether anyone is
//! using it. Read by the control plane's trial gate and by the workspace's
//! own first-run measurement; the thresholds live with the caller, this
//! module reports only what the domain tables hold.
//!
//! Every read here is on FORCE-RLS tenant tables, so the caller MUST run it
//! with the workspace pinned (`with_actor_context` / `TenantConn`). Nothing is
//! pushed anywhere; the state is computed on request from the workspace's
//! own rows.

use chrono::{DateTime, Utc};
use diesel::dsl::{count_star, max, min};
use diesel::prelude::*;
use serde::Serialize;

use crate::db::DbConnection;

/// Roles whose comments count as an agent reply. Mirrors the staff set in
/// `workspaces.rs`; a requester replying to their own ticket is not value
/// delivered.
const STAFF_ROLES: [&str; 3] = ["owner", "admin", "agent"];

/// Which integrations the workspace has configured. Booleans where at most
/// one row exists per workspace, counts where many can.
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct ActivationIntegrations {
    /// An inbound channel (mailbox) is enabled.
    pub inbound_channel: bool,
    /// Outbound email is configured and enabled.
    pub outbound_email: bool,
    pub ldap: bool,
    /// At least one Microsoft Graph sync has run.
    pub msgraph: bool,
    pub webhooks: i64,
    /// Plugins in the `installed` state.
    pub plugins: i64,
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct ActivationState {
    pub first_ticket_at: Option<DateTime<Utc>>,
    /// First public comment by a staff member.
    pub first_agent_reply_at: Option<DateTime<Utc>>,
    /// First accepted membership beyond the owner.
    pub first_member_joined_at: Option<DateTime<Utc>>,
    /// Latest ticket or page update.
    pub last_activity_at: Option<DateTime<Utc>>,
    pub tickets: i64,
    /// Active memberships, owner included.
    pub members: i64,
    /// Live pages outside the seeded system collection.
    pub documents: i64,
    pub integrations: ActivationIntegrations,
}

/// Compute the activation state of the pinned workspace.
pub fn activation_state(conn: &mut DbConnection) -> QueryResult<ActivationState> {
    use crate::schema::{
        channels, comments, documentation_collection_pages, documentation_collections,
        documentation_pages, plugins, sync_history, tickets, webhooks, workspace_email_settings,
        workspace_ldap_settings, workspace_members,
    };

    let (first_ticket_at, last_ticket_update, tickets): (
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
        i64,
    ) = tickets::table
        .select((
            min(tickets::created_at),
            max(tickets::updated_at),
            count_star(),
        ))
        .first(conn)?;

    let first_agent_reply_at: Option<DateTime<Utc>> = comments::table
        .inner_join(
            workspace_members::table.on(workspace_members::user_uuid
                .eq(comments::user_uuid)
                .and(workspace_members::workspace_id.eq(comments::workspace_id))),
        )
        .filter(comments::is_internal.eq(false))
        .filter(comments::deleted_at.is_null())
        .filter(workspace_members::role.eq_any(STAFF_ROLES))
        .select(min(comments::created_at))
        .first(conn)?;

    let (members, first_member_joined_at): (i64, Option<DateTime<Utc>>) = {
        let active = workspace_members::table.filter(workspace_members::removed_at.is_null());
        let members: i64 = active.select(count_star()).first(conn)?;
        let joined: Option<DateTime<Utc>> = active
            .filter(workspace_members::role.ne("owner"))
            .select(min(workspace_members::accepted_at))
            .first(conn)?;
        (members, joined)
    };

    let (documents, last_page_update): (i64, Option<DateTime<Utc>>) = {
        let seeded = documentation_collection_pages::table
            .inner_join(documentation_collections::table)
            .filter(documentation_collections::is_system.eq(true))
            .select(documentation_collection_pages::page_id);
        documentation_pages::table
            .filter(documentation_pages::deleted_at.is_null())
            .filter(documentation_pages::archived_at.is_null())
            .filter(documentation_pages::id.ne_all(seeded))
            .select((count_star(), max(documentation_pages::updated_at)))
            .first(conn)?
    };

    let inbound_channel: i64 = channels::table
        .filter(channels::enabled.eq(true))
        .select(count_star())
        .first(conn)?;
    let outbound_email: i64 = workspace_email_settings::table
        .filter(workspace_email_settings::enabled.eq(true))
        .select(count_star())
        .first(conn)?;
    let ldap: i64 = workspace_ldap_settings::table
        .filter(workspace_ldap_settings::enabled.eq(true))
        .select(count_star())
        .first(conn)?;
    let msgraph: i64 = sync_history::table.select(count_star()).first(conn)?;
    let webhooks: i64 = webhooks::table.select(count_star()).first(conn)?;
    let installed_plugins: i64 = plugins::table
        .filter(plugins::state.eq("installed"))
        .select(count_star())
        .first(conn)?;

    Ok(ActivationState {
        first_ticket_at,
        first_agent_reply_at,
        first_member_joined_at,
        last_activity_at: last_ticket_update.max(last_page_update),
        tickets,
        members,
        documents,
        integrations: ActivationIntegrations {
            inbound_channel: inbound_channel > 0,
            outbound_email: outbound_email > 0,
            ldap: ldap > 0,
            msgraph: msgraph > 0,
            webhooks,
            plugins: installed_plugins,
        },
    })
}
