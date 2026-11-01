//! Who may use a plugin: its runtime endpoints (storage, collections, events,
//! the network proxy, runtime settings, the bundle token), the enabled-plugin
//! list, and its sync records (`sync::visibility`).
//!
//! The manifest's component `audience` decides who the app shows a component
//! to. A plugin's endpoints serve every one of its components, so they admit
//! the widest audience among them: a plugin whose components are all staff-only
//! refuses a requester, and one whose components are all admin-only refuses an
//! agent. A plugin with no components has nothing to show anyone, so it admits
//! staff only. The plugin's consented permissions are checked separately
//! (`handlers::plugins::authorize_plugin_data_request`); both must pass.

use crate::extractors::AuthContext;
use crate::models::Plugin;
use crate::models::PluginComponentAudience;

/// The caller's tier, as the manifest's audiences name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PluginCaller {
    Requester,
    Staff,
    Admin,
}

impl PluginCaller {
    pub fn of(auth: &AuthContext) -> Self {
        Self::from_tier(auth.is_workspace_admin(), auth.can_handle_tickets())
    }

    /// From the workspace-admin and staff flags, as a sync viewer carries them.
    pub fn from_tier(is_admin: bool, is_staff: bool) -> Self {
        if is_admin {
            PluginCaller::Admin
        } else if is_staff {
            PluginCaller::Staff
        } else {
            PluginCaller::Requester
        }
    }
}

/// The least tier an audience admits.
fn floor(audience: PluginComponentAudience) -> PluginCaller {
    match audience {
        PluginComponentAudience::All => PluginCaller::Requester,
        PluginComponentAudience::Staff => PluginCaller::Staff,
        PluginComponentAudience::Admin => PluginCaller::Admin,
    }
}

/// Whether `caller` meets the widest of `audiences` (staff when there are none).
pub fn admits(
    audiences: impl IntoIterator<Item = PluginComponentAudience>,
    caller: PluginCaller,
) -> bool {
    let least = audiences
        .into_iter()
        .map(floor)
        .min()
        .unwrap_or(PluginCaller::Staff);
    caller >= least
}

/// Whether `caller` may use `plugin`'s runtime endpoints. A manifest that
/// doesn't parse admits no one.
pub fn plugin_admits(plugin: &Plugin, caller: PluginCaller) -> bool {
    match plugin.parse_manifest() {
        Ok(manifest) => admits(manifest.components.values().map(|c| c.audience), caller),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PluginCaller::*;
    use PluginComponentAudience as A;

    #[test]
    fn the_widest_component_audience_decides() {
        assert!(admits([A::Staff, A::All], Requester));
        assert!(!admits([A::Staff, A::Admin], Requester));
        assert!(admits([A::Staff, A::Admin], Staff));
        assert!(!admits([A::Admin], Staff));
        assert!(admits([A::Admin], Admin));
        assert!(admits([A::All], Admin));
    }

    #[test]
    fn a_plugin_with_no_components_admits_staff_only() {
        assert!(!admits([], Requester));
        assert!(admits([], Staff));
        assert!(admits([], Admin));
    }
}
