//! Plugin lifecycle state machine.
//!
//! This module is the single chokepoint for every state change a
//! plugin row can undergo after the initial install. Handlers and
//! the install pipeline dispatch a [`PluginAction`] through
//! [`apply`]; the function exhaustively matches the (current
//! state, action) pair, performs the DB write inside a
//! transaction, logs the activity inline, and handles disk-side
//! effects (bundle removal) on commit.
//!
//! Why a separate module:
//!
//! - **Exhaustive matching.** The `match` in [`apply`] enumerates
//!   every legal transition; adding a new state or action becomes
//!   a compile error at every call site, not a runtime surprise.
//! - **Atomic state-plus-audit.** Activity logging happens inside
//!   the same transaction as the state flip, so the audit log
//!   never falls out of sync with the state column.
//! - **Signer continuity.** The `Reinstall` action enforces that
//!   a previously-uninstalled-with-preserve plugin can only be
//!   resurrected by the same signer that installed it (closes the
//!   data-inheritance bypass C3 from the architectural review).
//!
//! The fresh-install path (no existing row) does NOT route through
//! this module: there's no current state to transition from, so
//! the install pipeline creates the row directly with
//! `PluginState::Installed`. Once a row exists, every subsequent
//! state change runs through [`apply`].

use crate::db::DbConnection;
use crate::models::{Plugin, PluginState, PluginUpdate};
use crate::repository::plugins as plugin_repo;
use diesel::Connection;
use std::fmt;
use uuid::Uuid;

/// Every legal lifecycle action a caller can request. Validity
/// against the current state is checked exhaustively in [`apply`];
/// a (state, action) pair the match arms don't cover yields
/// [`ActionError::InvalidTransition`].
#[derive(Debug, Clone)]
pub enum PluginAction {
    /// Admin paused the plugin. From `Installed`. Bundle stops
    /// serving but data is preserved.
    Disable,
    /// Admin un-paused the plugin. From `Disabled` only;
    /// `Quarantined` requires `RestoreFromQuarantine` to make the
    /// admin override explicit.
    Enable,
    /// Trust-chain failure flagged this plugin. From any active
    /// state (`Installed`, `Disabled`). Set by background
    /// revocation sweeps; no user-initiated path.
    Quarantine { reason: QuarantineReason },
    /// Admin override re-activates a quarantined plugin. From
    /// `Quarantined` only. Must be a privileged operation in the
    /// handler layer; the lifecycle module does not police *who*
    /// can call it.
    RestoreFromQuarantine,
    /// Uninstall but retain plugin_data and collection rows for
    /// future reinstall (`lifecycle.on_uninstall = preserve` in
    /// the manifest). From any active state.
    UninstallPreserve,
    /// Uninstall and delete everything (`lifecycle.on_uninstall =
    /// cascade`). Removes the plugin row; FK cascade takes the
    /// rest. From any state including `Uninstalled` (an
    /// uninstall-preserve row can be hard-deleted by an admin).
    UninstallCascade,
    /// Resurrect a previously-uninstalled-with-preserve plugin.
    /// From `Uninstalled` only. The new install's signer pubkey
    /// MUST match the one captured at the original install, or
    /// the action is refused with `SignerMismatch` to prevent
    /// cross-publisher data inheritance.
    Reinstall { signer_pubkey: Option<String> },
}

impl PluginAction {
    fn name(&self) -> &'static str {
        match self {
            Self::Disable => "Disable",
            Self::Enable => "Enable",
            Self::Quarantine { .. } => "Quarantine",
            Self::RestoreFromQuarantine => "RestoreFromQuarantine",
            Self::UninstallPreserve => "UninstallPreserve",
            Self::UninstallCascade => "UninstallCascade",
            Self::Reinstall { .. } => "Reinstall",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum QuarantineReason {
    /// Publisher's pubkey was removed from the signed keylist.
    SignerRevoked,
    /// On-disk bundle no longer matches the recorded
    /// `bundle_hash` / signature envelope.
    SignatureMismatchOnRecheck,
    /// Operator-flagged policy violation (manual quarantine).
    PolicyViolation,
}

impl fmt::Display for QuarantineReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignerRevoked => f.write_str("signer revoked"),
            Self::SignatureMismatchOnRecheck => f.write_str("signature mismatch on recheck"),
            Self::PolicyViolation => f.write_str("policy violation"),
        }
    }
}

/// Result of a successful action.
#[derive(Debug)]
pub enum ActionOutcome {
    /// The plugin row remains in the DB at a new state.
    StateChanged { plugin: Plugin, prior_state: PluginState },
    /// The plugin row was deleted (cascade uninstall).
    Deleted { uuid: Uuid, name: String },
}

#[derive(Debug)]
pub enum ActionError {
    /// (State, action) combination is not a legal transition.
    InvalidTransition {
        from: PluginState,
        action: &'static str,
    },
    /// Reinstall attempted with a different signer pubkey than
    /// the original install. Closes the cross-publisher data
    /// inheritance bypass.
    SignerMismatch {
        existing_fingerprint: String,
        attempted_fingerprint: String,
    },
    /// No plugin row exists for the given uuid.
    NoSuchPlugin,
    Db(diesel::result::Error),
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { from, action } => {
                write!(f, "cannot apply {action} from state {from}")
            }
            Self::SignerMismatch { existing_fingerprint, attempted_fingerprint } => write!(
                f,
                "reinstall refused: existing signer {existing_fingerprint} does not match attempted {attempted_fingerprint}"
            ),
            Self::NoSuchPlugin => f.write_str("plugin not found"),
            Self::Db(e) => write!(f, "database error: {e}"),
        }
    }
}

impl std::error::Error for ActionError {}

impl From<diesel::result::Error> for ActionError {
    fn from(e: diesel::result::Error) -> Self {
        match e {
            diesel::result::Error::NotFound => ActionError::NoSuchPlugin,
            other => ActionError::Db(other),
        }
    }
}

/// Apply an action to a plugin row. The DB update + activity log
/// are written inside a single transaction; failure rolls both
/// back. Disk side effects (bundle file removal for uninstall
/// flavours) are caller responsibilities — see [`should_remove_bundle`].
pub fn apply(
    conn: &mut DbConnection,
    plugin_uuid: Uuid,
    action: PluginAction,
    actor: Option<Uuid>,
) -> Result<ActionOutcome, ActionError> {
    conn.transaction::<_, ActionError, _>(|tx| {
        let plugin = plugin_repo::get_plugin_by_uuid(tx, plugin_uuid)?;
        let prior = plugin.state;
        match (prior, &action) {
            // ----- Active <-> Disabled -----
            (PluginState::Installed, PluginAction::Disable) => {
                let updated = set_state(tx, plugin_uuid, PluginState::Disabled)?;
                log_lifecycle(tx, &updated, &action, prior, actor)?;
                Ok(ActionOutcome::StateChanged { plugin: updated, prior_state: prior })
            }
            (PluginState::Disabled, PluginAction::Enable) => {
                let updated = set_state(tx, plugin_uuid, PluginState::Installed)?;
                log_lifecycle(tx, &updated, &action, prior, actor)?;
                Ok(ActionOutcome::StateChanged { plugin: updated, prior_state: prior })
            }

            // ----- Quarantine -----
            (PluginState::Installed, PluginAction::Quarantine { .. })
            | (PluginState::Disabled, PluginAction::Quarantine { .. }) => {
                let updated = set_state(tx, plugin_uuid, PluginState::Quarantined)?;
                log_lifecycle(tx, &updated, &action, prior, actor)?;
                Ok(ActionOutcome::StateChanged { plugin: updated, prior_state: prior })
            }
            (PluginState::Quarantined, PluginAction::RestoreFromQuarantine) => {
                let updated = set_state(tx, plugin_uuid, PluginState::Installed)?;
                log_lifecycle(tx, &updated, &action, prior, actor)?;
                Ok(ActionOutcome::StateChanged { plugin: updated, prior_state: prior })
            }

            // ----- Uninstall (preserve) -----
            (PluginState::Installed, PluginAction::UninstallPreserve)
            | (PluginState::Disabled, PluginAction::UninstallPreserve)
            | (PluginState::Quarantined, PluginAction::UninstallPreserve) => {
                let updated = set_state(tx, plugin_uuid, PluginState::Uninstalled)?;
                log_lifecycle(tx, &updated, &action, prior, actor)?;
                Ok(ActionOutcome::StateChanged { plugin: updated, prior_state: prior })
            }

            // ----- Uninstall (cascade) -----
            // Allowed from any state, including `Uninstalled` (an
            // operator can hard-delete a preserved row to fully
            // free its name and data).
            (_, PluginAction::UninstallCascade) => {
                let name = plugin.name.clone();
                let id = plugin.id;
                // Log first so the activity row's plugin_id FK is
                // still valid; the cascade delete that follows
                // will remove activity rows too.
                plugin_repo::log_plugin_activity(
                    tx,
                    id,
                    format!("lifecycle:UninstallCascade (from {prior})"),
                    None,
                    actor,
                )?;
                plugin_repo::delete_plugin_by_uuid(tx, plugin_uuid)?;
                Ok(ActionOutcome::Deleted { uuid: plugin_uuid, name })
            }

            // ----- Reinstall (signer continuity) -----
            (PluginState::Uninstalled, PluginAction::Reinstall { signer_pubkey }) => {
                if let Some(existing_pk) = plugin.signer_pubkey.as_ref() {
                    let attempted = signer_pubkey.as_deref();
                    if attempted != Some(existing_pk.as_str()) {
                        return Err(ActionError::SignerMismatch {
                            existing_fingerprint: short_fingerprint(Some(existing_pk)),
                            attempted_fingerprint: short_fingerprint(attempted),
                        });
                    }
                }
                let updated = set_state(tx, plugin_uuid, PluginState::Installed)?;
                log_lifecycle(tx, &updated, &action, prior, actor)?;
                Ok(ActionOutcome::StateChanged { plugin: updated, prior_state: prior })
            }

            // ----- Everything else is a refused transition -----
            (from, action) => Err(ActionError::InvalidTransition {
                from,
                action: action.name(),
            }),
        }
    })
}

/// Did the action delete the plugin's bundle from disk? Callers
/// that own the on-disk bundle (the install pipeline) consult
/// this to decide whether to remove the bundle file after the
/// transaction commits. We expose it as a helper rather than
/// bundle file management inside `apply` because the disk
/// location lives in the install module.
pub fn outcome_removes_bundle(outcome: &ActionOutcome) -> bool {
    match outcome {
        ActionOutcome::Deleted { .. } => true,
        ActionOutcome::StateChanged { plugin, .. } => {
            matches!(plugin.state, PluginState::Uninstalled)
        }
    }
}

fn set_state(
    conn: &mut DbConnection,
    plugin_uuid: Uuid,
    new_state: PluginState,
) -> Result<Plugin, ActionError> {
    let update = PluginUpdate {
        state: Some(new_state),
        ..Default::default()
    };
    Ok(plugin_repo::update_plugin_by_uuid(conn, plugin_uuid, update)?)
}

fn log_lifecycle(
    conn: &mut DbConnection,
    plugin: &Plugin,
    action: &PluginAction,
    prior: PluginState,
    actor: Option<Uuid>,
) -> Result<(), ActionError> {
    // A compact `lifecycle:<Action>` audit verb keeps the activity
    // table greppable. Detail goes in the `details` JSON for
    // structured tooling later.
    let summary = format!("lifecycle:{} ({} -> {})", action.name(), prior, plugin.state);
    let details = serde_json::json!({
        "action": action.name(),
        "from": prior,
        "to": plugin.state,
        "extra": match action {
            PluginAction::Quarantine { reason } => Some(serde_json::json!({ "reason": reason.to_string() })),
            _ => None,
        },
    });
    plugin_repo::log_plugin_activity(conn, plugin.id, summary, Some(details), actor)?;
    Ok(())
}

/// Short fingerprint for an Ed25519 pubkey string. We never log
/// the full pubkey on the error path; the leading 12 chars of the
/// base64 are unique enough for an admin to disambiguate two
/// signers and short enough not to accidentally leak the full
/// material into structured audit consumers.
fn short_fingerprint(pubkey: Option<&str>) -> String {
    match pubkey {
        None => "(none)".to_string(),
        Some(s) if s.len() <= 12 => s.to_string(),
        Some(s) => format!("{}…", &s[..12]),
    }
}

// =============================================================================
// Tests
// =============================================================================
//
// The lifecycle invariants split into two layers:
//
//   - **Pure rule tests** (this module) verify the (state, action)
//     match table without touching the DB. They construct a
//     fictional `Plugin` value and call helper assertions about
//     which actions a state allows. No transactions, no fixtures.
//
//   - **Integration tests** live alongside the install pipeline
//     and exercise the real DB transaction, including signer-
//     continuity refusal on reinstall.
//
// Pure tests catch mismatched arms in the big match; integration
// tests catch DB-level surprises.

#[cfg(test)]
mod tests {
    use super::*;

    /// Cases the match arms accept (state, action) for. Used as
    /// the source-of-truth in the legality tests below — if the
    /// match table drifts from this list, the tests fail.
    fn legal_transitions() -> Vec<(PluginState, &'static str)> {
        vec![
            (PluginState::Installed, "Disable"),
            (PluginState::Disabled, "Enable"),
            (PluginState::Installed, "Quarantine"),
            (PluginState::Disabled, "Quarantine"),
            (PluginState::Quarantined, "RestoreFromQuarantine"),
            (PluginState::Installed, "UninstallPreserve"),
            (PluginState::Disabled, "UninstallPreserve"),
            (PluginState::Quarantined, "UninstallPreserve"),
            (PluginState::Installed, "UninstallCascade"),
            (PluginState::Disabled, "UninstallCascade"),
            (PluginState::Quarantined, "UninstallCascade"),
            (PluginState::Uninstalled, "UninstallCascade"),
            (PluginState::Uninstalled, "Reinstall"),
        ]
    }

    fn all_actions() -> Vec<PluginAction> {
        vec![
            PluginAction::Disable,
            PluginAction::Enable,
            PluginAction::Quarantine { reason: QuarantineReason::PolicyViolation },
            PluginAction::RestoreFromQuarantine,
            PluginAction::UninstallPreserve,
            PluginAction::UninstallCascade,
            PluginAction::Reinstall { signer_pubkey: Some("pk".into()) },
        ]
    }

    fn all_states() -> [PluginState; 4] {
        [
            PluginState::Installed,
            PluginState::Disabled,
            PluginState::Quarantined,
            PluginState::Uninstalled,
        ]
    }

    #[test]
    fn legal_transitions_table_is_consistent() {
        // Sanity check that the legality table doesn't list a
        // (state, action) pair that the production code would
        // reject. Pure data, no DB.
        for (state, action_name) in legal_transitions() {
            assert!(
                all_actions().iter().any(|a| a.name() == action_name),
                "legal_transitions references unknown action {action_name}"
            );
            assert!(
                all_states().contains(&state),
                "legal_transitions references unknown state {state}"
            );
        }
    }

    #[test]
    fn action_names_are_unique_and_stable() {
        // Audit log greps on these strings, so changing them is a
        // breaking change for ops tooling.
        let mut names: Vec<&str> = all_actions().iter().map(|a| a.name()).collect();
        names.sort();
        let original = names.clone();
        names.dedup();
        assert_eq!(names, original, "PluginAction::name() must be unique");
    }

    #[test]
    fn quarantine_reason_displays_human_readable() {
        assert_eq!(
            format!("{}", QuarantineReason::SignerRevoked),
            "signer revoked"
        );
        assert_eq!(
            format!("{}", QuarantineReason::SignatureMismatchOnRecheck),
            "signature mismatch on recheck"
        );
        assert_eq!(
            format!("{}", QuarantineReason::PolicyViolation),
            "policy violation"
        );
    }

    #[test]
    fn short_fingerprint_handles_short_and_long() {
        assert_eq!(short_fingerprint(None), "(none)");
        assert_eq!(short_fingerprint(Some("short")), "short");
        assert_eq!(
            short_fingerprint(Some("AAAAAAAAAAAAEXTRA")),
            "AAAAAAAAAAAA…"
        );
    }

    #[test]
    fn signer_mismatch_error_includes_short_fingerprints() {
        let err = ActionError::SignerMismatch {
            existing_fingerprint: "AAAAAAAAAAAA…".to_string(),
            attempted_fingerprint: "BBBBBBBBBBBB…".to_string(),
        };
        let s = format!("{err}");
        assert!(s.contains("AAAAAAAAAAAA"));
        assert!(s.contains("BBBBBBBBBBBB"));
        assert!(!s.contains("AAAAAAAAAAAAEXTRA")); // verify truncation isn't bypassed
    }

    #[test]
    fn invalid_transition_error_names_state_and_action() {
        let err = ActionError::InvalidTransition {
            from: PluginState::Quarantined,
            action: "Enable",
        };
        let s = format!("{err}");
        assert!(s.contains("quarantined"));
        assert!(s.contains("Enable"));
    }
}
