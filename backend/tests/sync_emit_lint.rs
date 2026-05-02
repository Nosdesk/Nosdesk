//! Lint: every repository write either calls `sync::emit::record` or
//! is on the audit-only allowlist. Runs as a `cargo test` integration
//! test so CI catches a missed emit on new repository writes.
//!
//! The allowlist is the set of currently-unwired writes — Phase 2
//! ships with most of the existing surface area on the list and
//! shrinks it as each aggregate gets wired through `emit::record`.
//! Adding a NEW unwired write should be intentional: either wire the
//! emit, or update the allowlist with a comment explaining why the
//! write is audit-only (e.g. operational tables that no sync client
//! consumes).

use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

/// Seeded with the existing repository write surface area. Every
/// entry is a write fn that doesn't yet emit a sync_action either
/// because (a) its underlying table is intentionally audit-only
/// (operational / bespoke / Yjs / security event) or (b) it's a
/// tier-1 aggregate pending an emit-wiring commit later in Phase 2.
///
/// Removing an entry from the (b) section is part of the commit that
/// wires the matching emit. Adding a NEW entry should require an
/// inline justification in the diff.
const ALLOWLIST: &[&str] = &[
    // ----- Audit-only or bespoke (intentionally not in sync registry) -----
    "repository/active_sessions.rs::cleanup_expired",
    "repository/active_sessions.rs::revoke_other_sessions",
    "repository/active_sessions.rs::revoke_other_sessions_by_uuid",
    "repository/active_sessions.rs::revoke_session_by_uuid",
    "repository/active_sessions.rs::update_session_activity",
    "repository/api_tokens.rs::create_api_token",
    "repository/api_tokens.rs::revoke_api_token",
    "repository/api_tokens.rs::update_token_last_used",
    "repository/article_content.rs::create_article_content_revision",
    "repository/article_content.rs::increment_article_content_revision",
    "repository/article_content.rs::update_article_yjs_state",
    "repository/article_content.rs::update_ticket_modified_timestamp",
    "repository/backup.rs::create_backup_job",
    "repository/backup.rs::delete_backup_job",
    "repository/backup.rs::update_backup_job",
    "repository/feature_flags.rs::set_workspace_flag",
    "repository/feature_flags.rs::set_user_override",
    "repository/feature_flags.rs::set_all_workspace_flags",
    "repository/passkey_credentials.rs::create",
    "repository/passkey_credentials.rs::delete_for_user",
    "repository/passkey_credentials.rs::update_for_user",
    "repository/plugin_publishers.rs::insert_local_signing_key",
    "repository/plugin_publishers.rs::update_registry_state",
    "repository/refresh_tokens.rs::cleanup_expired",
    "repository/refresh_tokens.rs::mark_token_used",
    "repository/refresh_tokens.rs::revoke_token_family",
    "repository/reset_tokens.rs::invalidate_tokens_by_type",
    "repository/reset_tokens.rs::mark_token_as_used",
    "repository/search_query_log.rs::log_query",
    "repository/search_query_log.rs::prune_old_rows",
    "repository/site_settings.rs::update_favicon_url",
    "repository/site_settings.rs::update_logo_light_url",
    "repository/site_settings.rs::update_logo_url",
    "repository/sync_history.rs::create_sync_history",
    "repository/sync_history.rs::update_sync_history",
    "repository/user_helpers.rs::create_user_with_email",
    "repository/user_ticket_views.rs::delete_view",

    // ----- Tier-1 aggregates pending an emit-wiring commit -----
    "repository/canned_responses.rs::create",
    "repository/canned_responses.rs::delete",
    "repository/canned_responses.rs::update",
    "repository/categories.rs::set_category_visibility",
    "repository/categories.rs::update_category_orders",
    "repository/channels.rs::create",
    "repository/channels.rs::delete",
    "repository/channels.rs::delete_credential",
    "repository/channels.rs::put_credential",
    "repository/channels.rs::update",
    "repository/channels.rs::update_runtime_state",
    "repository/comments.rs::create_attachment",
    "repository/comments.rs::delete_attachment",
    "repository/documentation_collections.rs::add_page_to_collection_at_root",
    "repository/documentation_collections.rs::cascade_collection_membership",
    "repository/documentation_collections.rs::reorder_collections",
    "repository/documentation_collections.rs::soft_delete_pages_in_collection",
    "repository/documentation_collections.rs::update_collection_description_yjs",
    "repository/documentation_page_tickets.rs::delete_link",
    "repository/documentation_page_tickets.rs::upsert_link",
    "repository/documentation_subscriptions.rs::subscribe_user",
    "repository/documentation_subscriptions.rs::unsubscribe_user",
    "repository/documentation.rs::create_documentation_page",
    "repository/documentation.rs::create_documentation_revision",
    "repository/documentation.rs::delete_documentation_page",
    "repository/documentation.rs::move_page_to_parent",
    "repository/documentation.rs::permanently_delete_page",
    "repository/documentation.rs::reorder_pages",
    "repository/documentation.rs::sync_page_embeddings",
    "repository/documentation.rs::update_documentation_page",
    "repository/documentation.rs::update_documentation_yjs_state",
    "repository/groups.rs::add_device_to_group",
    "repository/groups.rs::mark_groups_not_synced",
    "repository/groups.rs::remove_device_from_group",
    "repository/groups.rs::set_group_devices",
    "repository/groups.rs::set_group_includes",
    "repository/groups.rs::set_group_members",
    "repository/groups.rs::set_user_groups",
    "repository/groups.rs::unmanage_group",
    "repository/groups.rs::upsert_external_group",
    "repository/knowledge_gaps.rs::attach_signal",
    "repository/knowledge_gaps.rs::create_gap",
    "repository/knowledge_gaps.rs::dismiss_signal",
    "repository/knowledge_gaps.rs::update_gap",
    "repository/plugin_collections.rs::create_row",
    "repository/plugin_collections.rs::create_schema",
    "repository/plugin_collections.rs::delete_schema",
    "repository/plugin_collections.rs::list_rows",
    "repository/plugin_collections.rs::update_schema",
    "repository/projects.rs::update_project_ticket_orders",
    "repository/tickets.rs::add_device_to_ticket",
    "repository/tickets.rs::remove_device_from_ticket",
    "repository/tickets.rs::verify_pending_tickets_for_user",
    "repository/user_auth_identities.rs::update_local_password_hash",
    "repository/user_emails.rs::add_multiple_emails",
    "repository/user_emails.rs::cleanup_obsolete_emails",
    "repository/users.rs::clear_user_mfa",
    "repository/users.rs::update_user_mfa",
    "repository/webhooks.rs::create_delivery",
    "repository/webhooks.rs::delete_webhook_by_uuid",
    "repository/webhooks.rs::update_delivery",
    "repository/webhooks.rs::update_webhook_by_uuid",

    // Continued seed — top-level CRUD entry points pending wiring.
    "repository/active_sessions.rs::create_session",
    "repository/active_sessions.rs::revoke_session",
    "repository/article_content.rs::create_article_content",
    "repository/assignment_rules.rs::create_rule",
    "repository/assignment_rules.rs::delete_rule",
    "repository/assignment_rules.rs::reorder_rules",
    "repository/assignment_rules.rs::update_rule",
    "repository/categories.rs::create_category",
    "repository/categories.rs::delete_category",
    "repository/categories.rs::update_category",
    "repository/channels.rs::record_message",
    "repository/comments.rs::create_comment",
    "repository/comments.rs::delete_comment",
    "repository/devices.rs::create_device",
    "repository/devices.rs::delete_device",
    "repository/devices.rs::update_device",
    "repository/documentation_collections.rs::add_page_to_collection",
    "repository/documentation_collections.rs::create_collection",
    "repository/documentation_collections.rs::delete_collection",
    "repository/documentation_collections.rs::remove_page_from_collection",
    "repository/documentation_collections.rs::set_collection_visibility",
    "repository/documentation_collections.rs::update_collection",
    "repository/documentation_starred_pages.rs::star_page",
    "repository/documentation_starred_pages.rs::unstar_page",
    "repository/documentation.rs::set_page_visibility",
    "repository/groups.rs::add_group_include",
    "repository/groups.rs::add_user_to_group",
    "repository/groups.rs::create_group",
    "repository/groups.rs::delete_group",
    "repository/groups.rs::remove_group_include",
    "repository/groups.rs::remove_user_from_group",
    "repository/groups.rs::update_group",
    "repository/linked_tickets.rs::link_tickets",
    "repository/linked_tickets.rs::unlink_tickets",
    "repository/plugin_collections.rs::delete_row",
    "repository/plugin_collections.rs::update_row",
    "repository/plugin_publishers.rs::revoke_publisher",
    "repository/plugin_publishers.rs::upsert_publisher",
    "repository/projects.rs::add_ticket_to_project",
    "repository/projects.rs::create_project",
    "repository/projects.rs::delete_project",
    "repository/projects.rs::remove_ticket_from_project",
    "repository/projects.rs::update_project",
    "repository/refresh_tokens.rs::create_refresh_token",
    "repository/refresh_tokens.rs::revoke_refresh_token",
    "repository/reset_tokens.rs::create_reset_token",
    "repository/site_settings.rs::update_site_settings",
    "repository/sync_history.rs::delete_delta_token",
    "repository/sync_history.rs::upsert_delta_token",
    // create_ticket, update_ticket, update_ticket_partial, and
    // delete_ticket_with_cleanup are wired through emit::record.
    "repository/user_auth_identities.rs::create_identity",
    "repository/user_auth_identities.rs::delete_identity",
    "repository/user_ticket_views.rs::record_view",
    "repository/users.rs::create_user",
    "repository/users.rs::delete_user",
    "repository/users.rs::update_user",
    "repository/webhooks.rs::create_webhook",
    "repository/webhooks.rs::update_webhook",
];

#[derive(Debug)]
struct ReportEntry {
    relpath: String,
    fn_name: String,
}

#[test]
fn every_repository_write_calls_sync_emit_or_is_allowlisted() {
    let repo_root: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/repository");
    assert!(
        repo_root.exists(),
        "repository directory not found at {}",
        repo_root.display()
    );

    let allowlist: HashSet<String> = ALLOWLIST.iter().map(|s| s.to_string()).collect();
    let mut violations: Vec<ReportEntry> = Vec::new();

    let fn_re = Regex::new(r"(?m)^\s*pub(?:\s*\([^)]*\))?\s+fn\s+(\w+)\s*[<(]").unwrap();
    let write_re = Regex::new(
        r"diesel::insert_into\s*\(|diesel::update\s*\(|diesel::delete\s*\(|diesel::sql_query\s*\(",
    )
    .unwrap();

    for entry in WalkDir::new(&repo_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
    {
        let path = entry.path();
        let relpath = format!(
            "repository/{}",
            path.strip_prefix(&repo_root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        );
        // Skip the module root and the workflow_states module — workflow_states
        // is fully wired and the test should fail loudly if it ever regresses.
        // The mod.rs has no `pub fn` of its own.
        if relpath == "repository/mod.rs" {
            continue;
        }

        let src = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let src = strip_test_modules(&src);
        let src = strip_block_comments(&src);

        for func in iter_pub_fns(&src, &fn_re) {
            if !write_re.is_match(&func.body) {
                continue;
            }
            // Treat both qualified and unqualified calls as wired.
            let wired = func.body.contains("emit::record")
                || func.body.contains("sync::emit::record")
                || func.body.contains("sync_emit::record");
            if wired {
                continue;
            }
            let key = format!("{}::{}", relpath, func.name);
            if allowlist.contains(&key) {
                continue;
            }
            violations.push(ReportEntry {
                relpath: relpath.clone(),
                fn_name: func.name.clone(),
            });
        }
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "\nUnwired repository writes found. Either call sync::emit::record\n\
             in the same transaction, or add the function to the audit-only\n\
             allowlist in tests/sync_emit_lint.rs:\n\n",
        );
        for v in &violations {
            msg.push_str(&format!("  {}::{}\n", v.relpath, v.fn_name));
        }
        panic!("{msg}");
    }
}

struct PubFn {
    name: String,
    body: String,
}

/// Find every `pub fn` in the source and capture its body between
/// the matching `{` and `}`. Brace counting is a string scan; good
/// enough because Rust's grammar bans unbalanced braces inside
/// strings/chars after the syntax parser approves the file.
fn iter_pub_fns(src: &str, fn_re: &Regex) -> Vec<PubFn> {
    let mut out = Vec::new();
    for caps in fn_re.captures_iter(src) {
        let name = caps.get(1).unwrap().as_str().to_string();
        let header_end = caps.get(0).unwrap().end();
        let Some(open_brace) = src[header_end..].find('{') else {
            continue;
        };
        let body_start = header_end + open_brace + 1;
        let mut depth = 1usize;
        let mut i = body_start;
        let bytes = src.as_bytes();
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        let body = src[body_start..i.saturating_sub(1)].to_string();
        out.push(PubFn { name, body });
    }
    out
}

/// Drop `#[cfg(test)] mod tests { ... }` blocks so test fixtures
/// that intentionally bypass emit don't trip the lint. Blocks are
/// matched by the literal `#[cfg(test)]` attribute on a mod
/// declaration.
fn strip_test_modules(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let bytes = src.as_bytes();
    while i < bytes.len() {
        if let Some(attr_pos) = src[i..].find("#[cfg(test)]") {
            let absolute = i + attr_pos;
            out.push_str(&src[i..absolute]);
            // Find the next `mod NAME {` after the attribute.
            let after_attr = absolute + "#[cfg(test)]".len();
            let Some(mod_pos) = src[after_attr..].find("mod ") else {
                out.push_str(&src[absolute..]);
                break;
            };
            let abs_mod = after_attr + mod_pos;
            let Some(open) = src[abs_mod..].find('{') else {
                out.push_str(&src[absolute..]);
                break;
            };
            let body_start = abs_mod + open + 1;
            let mut depth = 1usize;
            let mut j = body_start;
            while j < bytes.len() && depth > 0 {
                match bytes[j] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            i = j;
        } else {
            out.push_str(&src[i..]);
            break;
        }
    }
    out
}

/// Strip /* ... */ block comments so a sample `diesel::update(` in a
/// docstring example doesn't trip the write detector. Doc comments
/// are line-style (`///`) so leaving those alone is safe.
fn strip_block_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let bytes = src.as_bytes();
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Find matching */, skip past it.
            if let Some(end) = src[i + 2..].find("*/") {
                i = i + 2 + end + 2;
                continue;
            }
            // Unterminated; bail and copy the rest verbatim.
            out.push_str(&src[i..]);
            break;
        }
        out.push(src[i..].chars().next().unwrap());
        i += src[i..].chars().next().unwrap().len_utf8();
    }
    out
}
