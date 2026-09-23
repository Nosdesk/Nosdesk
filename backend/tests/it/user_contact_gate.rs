//! Every contact handler runs the contact gate, reads included.
//!
//! `user_contact.rs` states its own contract at the top of the module:
//! "Profile reads/writes mirror the user-update gate (self or admin)". The
//! writes did. The three read handlers took `AuthContext`, bound it to `_auth`
//! and discarded it, so any member of a workspace could read any other
//! member's profile fields, phone numbers and postal addresses. The handlers
//! contradicted the module's own stated contract, which is the kind of thing
//! review does not catch because the signature looks right.
//!
//! Scoped to this module on purpose. A general "no handler discards
//! `AuthContext`" lint was measured and rejected: 43 handlers bind `_auth`,
//! and nearly all are legitimate, workspace-scoped resources where the
//! `TenantConn` RLS pin is the whole authorization and the extractor is
//! present only to require authentication. That lint would have meant ~42
//! exemption markers for one finding, the same reason the per-handler
//! authorization lint was rejected in `route_auth_funnel_lint`.

use std::fs;
use std::path::PathBuf;

use regex::Regex;

const GATE: &str = "guard_contact_access";

#[test]
fn contact_handlers_run_the_contact_gate() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = fs::read_to_string(manifest.join("src/handlers/user_contact.rs"))
        .expect("read user_contact.rs");

    // Handlers only: this module's helpers take neither extractor.
    let fn_re = Regex::new(r"(?m)^pub async fn (\w+)\(").expect("fn regex");
    let mut ungated: Vec<String> = Vec::new();

    for caps in fn_re.captures_iter(&src) {
        let name = caps.get(1).expect("name").as_str();
        let start = caps.get(0).expect("m").start();
        let end = src[start..]
            .find("\n}\n")
            .map(|i| start + i)
            .unwrap_or(src.len());
        let body = &src[start..end];

        // Only handlers with a per-person subject are in scope. The field-schema
        // handlers act on the workspace's schema, not on anyone's details, and
        // the module doc says schema reads are open to authenticated staff.
        if !body.contains("web::Path<Uuid>") {
            continue;
        }
        // A decision about the subject is either the shared gate or an explicit
        // comparison against it. Taking `AuthContext` and binding it to `_auth`
        // is neither, and is exactly what the reads used to do.
        if !body.contains(GATE) && !body.contains("auth.user_uuid != user_uuid") {
            ungated.push(name.to_string());
        }
    }

    assert!(
        ungated.is_empty(),
        "These contact handlers name a subject user in their path but make no \
         authorization decision about them, so they read or write one person's \
         contact details on another person's say-so:\n  {}\n\n\
         Call `{GATE}`, which is the gate the module's own doc comment describes.",
        ungated.join("\n  "),
    );
}
