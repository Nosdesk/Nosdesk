//! Group -> workspace-role mapping: derive a directory user's workspace role
//! from their group memberships.
//!
//! OPT-IN: a no-op unless `role_mappings` are configured in `group_config`. When
//! they are, the directory is AUTHORITATIVE for role -- each governed user's
//! role becomes the highest mapped role among their groups, or the configured
//! `default_role` (Member) if none match. This is the standard group-based
//! access model (a user removed from the admin group is downgraded next sync;
//! that's the point of opting in).
//!
//! Guard rails, so a misconfigured mapping can't lock anyone out:
//!   * only users WITH an `ldap` identity are governed -- a locally-created
//!     admin (no directory identity) is never touched;
//!   * an `owner` is never altered (owner is a deliberate, manual role);
//!   * `update_membership_role`'s own last-owner guard is a second backstop.
//!
//! Runs after the group sync (so memberships are fresh). Pure `role_for` is unit
//! tested; the apply loop is DB-backed. Reused by SCIM later.

use serde_json::Value;
use tracing::{info, warn};

use crate::db::DbConnection;
use crate::models::{WorkspaceLdapSettings, WorkspaceRole};
use crate::repository::workspaces::UpdateMembershipRoleResult;
use crate::repository::{groups as groups_repo, user_auth_identities, workspaces};
use crate::services::ldap::sync::SyncError;

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct RoleMapStats {
    /// Directory users evaluated.
    pub evaluated: usize,
    /// Roles actually changed.
    pub changed: usize,
    /// Users skipped because they're an owner.
    pub skipped_owner: usize,
    /// Per-user failures (e.g. seat limit reached on a promotion).
    pub errors: usize,
}

struct RoleRule {
    /// Group common name (CN), matched case-insensitively.
    group: String,
    role: WorkspaceRole,
}

/// The parsed `role_mappings` config. `None` means the feature is off.
struct RoleMapConfig {
    rules: Vec<RoleRule>,
    default_role: WorkspaceRole,
}

impl RoleMapConfig {
    fn from_group_config(gc: &Value) -> Option<Self> {
        let rules: Vec<RoleRule> = gc
            .get("role_mappings")?
            .as_array()?
            .iter()
            .filter_map(|m| {
                let group = m.get("group")?.as_str()?.trim().to_string();
                let role = WorkspaceRole::from_db(m.get("role")?.as_str()?);
                (!group.is_empty()).then_some(RoleRule { group, role })
            })
            .collect();
        if rules.is_empty() {
            return None;
        }
        let default_role = gc
            .get("default_role")
            .and_then(Value::as_str)
            .map(WorkspaceRole::from_db)
            .unwrap_or(WorkspaceRole::Member);
        Some(RoleMapConfig {
            rules,
            default_role,
        })
    }

    /// The role for a user given their group names: the highest mapped role
    /// among matches (case-insensitive), else the default.
    fn role_for(&self, group_names: &[String]) -> WorkspaceRole {
        let lower: Vec<String> = group_names.iter().map(|g| g.to_lowercase()).collect();
        self.rules
            .iter()
            .filter(|r| lower.contains(&r.group.to_lowercase()))
            .map(|r| r.role)
            .max()
            .unwrap_or(self.default_role)
    }
}

/// Apply group->role mapping for every directory user in the workspace. No-op
/// when unconfigured. `conn` MUST be pinned to the workspace.
pub fn apply_role_mappings(
    conn: &mut DbConnection,
    settings: &WorkspaceLdapSettings,
    workspace_id: i32,
) -> Result<RoleMapStats, SyncError> {
    let Some(cfg) = RoleMapConfig::from_group_config(&settings.group_config) else {
        return Ok(RoleMapStats::default());
    };

    let mut stats = RoleMapStats::default();
    for user_uuid in user_auth_identities::ldap_user_uuids(conn, workspace_id)? {
        stats.evaluated += 1;

        let current = workspaces::get_membership_role(conn, workspace_id, user_uuid)?;
        // Owner is a deliberate, manual role: never directory-managed.
        if current.as_deref() == Some("owner") {
            stats.skipped_owner += 1;
            continue;
        }

        let group_names: Vec<String> = match groups_repo::get_groups_for_user(conn, &user_uuid) {
            Ok(groups) => groups.into_iter().map(|g| g.name).collect(),
            Err(e) => {
                warn!(error = %e, %user_uuid, "ldap role mapping: load groups failed");
                stats.errors += 1;
                continue;
            }
        };
        let target = cfg.role_for(&group_names);
        if current.as_deref() == Some(target.as_str()) {
            continue; // already correct; skip the write (no audit churn)
        }

        match workspaces::update_membership_role(conn, workspace_id, user_uuid, target.as_str()) {
            Ok(UpdateMembershipRoleResult::Updated(_)) => {
                info!(%user_uuid, role = target.as_str(), "ldap role mapping applied");
                stats.changed += 1;
            }
            // Not a member (shouldn't happen for a synced user) / would orphan
            // the last owner: leave it, don't fail the run.
            Ok(_) => {}
            Err(e) if workspaces::is_seat_limit_violation(&e) => {
                warn!(%user_uuid, role = target.as_str(), "ldap role mapping: seat limit reached, role not raised");
                stats.errors += 1;
            }
            Err(e) => {
                warn!(error = %e, %user_uuid, "ldap role mapping: update failed");
                stats.errors += 1;
            }
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(v: serde_json::Value) -> RoleMapConfig {
        RoleMapConfig::from_group_config(&v).expect("configured")
    }

    #[test]
    fn unconfigured_is_off() {
        assert!(RoleMapConfig::from_group_config(&json!({})).is_none());
        assert!(RoleMapConfig::from_group_config(&json!({ "role_mappings": [] })).is_none());
    }

    #[test]
    fn highest_matching_role_wins() {
        let c = cfg(json!({
            "role_mappings": [
                { "group": "Agents", "role": "agent" },
                { "group": "Helpdesk-Admins", "role": "admin" },
            ]
        }));
        // In both groups -> the higher (admin) wins regardless of order.
        assert_eq!(
            c.role_for(&["Agents".into(), "Helpdesk-Admins".into()]),
            WorkspaceRole::Admin
        );
        // Case-insensitive match.
        assert_eq!(c.role_for(&["agents".into()]), WorkspaceRole::Agent);
        // No match -> the default (Member).
        assert_eq!(c.role_for(&["Random".into()]), WorkspaceRole::Member);
    }

    #[test]
    fn default_role_is_configurable() {
        let c = cfg(json!({
            "role_mappings": [{ "group": "Admins", "role": "admin" }],
            "default_role": "agent",
        }));
        assert_eq!(c.role_for(&["Nothing".into()]), WorkspaceRole::Agent);
        assert_eq!(c.role_for(&["Admins".into()]), WorkspaceRole::Admin);
    }
}
