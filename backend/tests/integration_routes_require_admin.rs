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

    // The routes are registered in main.rs, between these two comments.
    let main_rs = fs::read_to_string(manifest.join("src/main.rs")).expect("read main.rs");
    let start = main_rs
        .find("// Microsoft Graph API endpoints")
        .expect("Graph route block in main.rs");
    let end = start
        + main_rs[start..]
            .find("// File upload endpoint")
            .expect("end of the Graph route block");
    let routed = Regex::new(r"web::(?:get|post|put|patch|delete)\(\)\.to\(handlers::(\w+)\)")
        .expect("route regex");
    let handlers: Vec<String> = routed
        .captures_iter(&main_rs[start..end])
        .map(|c| c[1].to_string())
        .collect();
    assert!(
        !handlers.is_empty(),
        "found no Graph routes; the scanner has drifted from main.rs"
    );

    let modules: Vec<(&str, String)> = ["msgraph_integration.rs", "microsoft_graph.rs"]
        .into_iter()
        .map(|module| {
            let src = fs::read_to_string(manifest.join("src/handlers").join(module))
                .expect("read module");
            // Drop the test module, which may legitimately build bare claims.
            let src = src.split("#[cfg(test)]").next().unwrap_or(&src).to_string();
            (module, src)
        })
        .collect();

    for name in handlers {
        let Some((module, body)) = modules.iter().find_map(|(module, src)| {
            src.find(&format!("fn {name}("))
                .map(|start| (module, &src[start..]))
        }) else {
            panic!("{name} is routed but defined in neither Graph module");
        };
        let end = body.find("\n}\n").map(|i| i + 2).unwrap_or(body.len());
        let body = &body[..end];
        if body.contains(BARE_AUTH) && !body.contains("require_workspace_role") {
            offenders.push(format!("{module}::{name}"));
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
