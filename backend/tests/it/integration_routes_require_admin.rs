//! Every route on the Entra/Intune integration is workspace-admin only.
//!
//! The integration runs on the organisation's own Graph app credentials, so
//! reading its configuration, probing the live connection, or cancelling a
//! running directory sync are all admin actions. Eight of these handlers used
//! to check only that the caller was authenticated, which made them member-
//! reachable while the sibling Graph proxy in `microsoft_graph.rs` required
//! admin for the same credentials.
//!
//! Asserted at the source rather than over HTTP: the gate itself
//! (`require_workspace_role`) needs a resolved workspace and a membership row,
//! and what regresses in practice is a ninth handler being added with the weak
//! check copied from its neighbours.

use std::fs;
use std::path::PathBuf;

use regex::Regex;

/// Bare authentication: claims are read out of the request extensions and
/// nothing further is asked. Correct for a self-service route, wrong for every
/// route in these two modules.
const BARE_AUTH: &str = "req.extensions().get::<crate::models::Claims>()";

#[test]
fn graph_integration_handlers_require_workspace_admin() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut offenders: Vec<String> = Vec::new();
    let routed =
        Regex::new(r"web::(?:get|post|put|patch|delete)\(\)\.to\((\w+)\)").expect("route regex");

    for module in ["msgraph_integration.rs", "microsoft_graph.rs"] {
        let path = manifest.join("src/handlers").join(module);
        let src = fs::read_to_string(&path).expect("read module");
        // Drop the test module, which may legitimately build bare claims.
        let src = src.split("#[cfg(test)]").next().unwrap_or(&src).to_string();

        let handlers: Vec<String> = routed
            .captures_iter(&src)
            .map(|c| c[1].to_string())
            .collect();
        assert!(
            !handlers.is_empty(),
            "{module}: found no routes; the scanner has drifted from the config fn"
        );

        for name in handlers {
            let Some(start) = src.find(&format!("fn {name}(")) else {
                continue;
            };
            let body = &src[start..];
            let end = body.find("\n}\n").map(|i| i + 2).unwrap_or(body.len());
            let body = &body[..end];
            if body.contains(BARE_AUTH) && !body.contains("require_workspace_role") {
                offenders.push(format!("{module}::{name}"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "These Graph integration handlers gate on being authenticated rather than \
         on being a workspace admin, though they act through the organisation's \
         Graph credentials:\n  {}\n\nAdd:\n  \
         crate::utils::rbac::require_workspace_role(&req, crate::models::WorkspaceRole::Admin)",
        offenders.join("\n  ")
    );
}
