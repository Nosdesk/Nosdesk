//! Shared validator + renderer for `{{variable}}` substitution in
//! admin-authored templates (canned responses, email signatures).
//!
//! Each surface declares its own allow-list of variable names; this
//! module owns the regex, the unknown-variable check, and the
//! substituter so the two pieces never drift between caller and
//! template literal. The frontend mirror lives in
//! `frontend/src/services/cannedResponsesService.ts` for the
//! canned-response path and in the per-feature site that adds a
//! signature mirror; both use the same `{{ name }}` whitespace-
//! tolerant pattern.
//!
//! The substituter is deliberately a flat `&str` replace and not a
//! Handlebars-style template engine: admins don't author logic,
//! just slots, so the simpler shape rules out template-injection
//! and conditional-rendering bugs entirely.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::BTreeSet;

/// Variables the canned-response renderer substitutes at insert
/// time. Keep in sync with `cannedResponsesService.renderTemplate`
/// on the frontend; the backend validation rejects any `{{...}}`
/// not on this list at save time so an admin typo like
/// `{{custmer_name}}` fails fast rather than landing in a customer
/// reply verbatim.
pub const CANNED_RESPONSE_VARIABLES: &[&str] = &[
    "ticket_id",
    "ticket_title",
    "customer_name",
    "customer_first_name",
    "tech_name",
    "tech_first_name",
    "app_name",
];

/// Variables the outbound channel pipeline substitutes when
/// appending an agent's signature. Scoped to per-agent metadata +
/// the workspace name; deliberately omits ticket-scoped tokens
/// since a signature is boilerplate, not a templated reply. The
/// first-name flavour lets admins write "Cheers, Alex" without
/// the full last name in the auto-signed footer.
pub const SIGNATURE_VARIABLES: &[&str] =
    &["tech_name", "tech_first_name", "tech_email", "app_name"];

/// Variables the auto-acknowledgement renderer substitutes when
/// emitting the "we got your message" reply. No `tech_name`: the
/// auto-ack is system-authored, there's no agent on hand yet.
pub const AUTO_ACK_VARIABLES: &[&str] = &[
    "ticket_id",
    "ticket_title",
    "customer_name",
    "customer_first_name",
    "app_name",
];

/// Variables the rules engine substitutes in a `reply` action's body
/// when a rule is applied. Mirrors `CANNED_RESPONSE_VARIABLES` plus
/// `agent_name` as an alias for `tech_name`: the synthesis (Phase 1
/// decision 34) standardised on "Actions" / "agent" for the rules
/// surface while keeping the legacy `tech_*` tokens for parity with
/// canned responses, so admins copying a body between the two
/// surfaces never see a save rejected for a typo'd token. Phase 2
/// extends this list with event- and reply-scoped tokens; for v1
/// the manual-trigger surface alone is in scope.
pub const RULE_REPLY_VARIABLES: &[&str] = &[
    "ticket_id",
    "ticket_title",
    "customer_name",
    "customer_first_name",
    "tech_name",
    "tech_first_name",
    "agent_name",
    "agent_first_name",
    "app_name",
];

/// Take the first whitespace-separated token of a full name.
/// "Mary Jane Smith" → "Mary"; "Alex" → "Alex"; "" → "". Empty
/// input returns empty; the substitute helper leaves an empty
/// substitution in place so missing context is visible to the
/// reader rather than silently dropped.
pub fn first_name(full_name: &str) -> String {
    full_name
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string()
}

/// Whitespace-tolerant token matcher: `{{ name }}` and `{{name}}`
/// both match the same way the frontend mirror does.
static VARIABLE_TOKEN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}").expect("valid regex"));

/// Return the sorted, deduplicated set of `{{token}}` names in
/// `body` that aren't on the allow-list. Empty when every token
/// is recognised. Callers turn a non-empty return into a 400.
pub fn unknown_variables(body: &str, allowed: &[&str]) -> Vec<String> {
    let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
    let mut out: BTreeSet<String> = BTreeSet::new();
    for cap in VARIABLE_TOKEN_RE.captures_iter(body) {
        let name = &cap[1];
        if !allowed.contains(name) {
            out.insert(name.to_string());
        }
    }
    out.into_iter().collect()
}

/// Substitute every `{{name}}` in `body` with its value from
/// `vars`. Unknown tokens are left as-is so the rendered output
/// makes the typo visible (matches the frontend's
/// `renderTemplate` behaviour). Whitespace inside the braces is
/// tolerated.
pub fn substitute(body: &str, vars: &[(&str, &str)]) -> String {
    VARIABLE_TOKEN_RE
        .replace_all(body, |caps: &regex::Captures<'_>| {
            let name = &caps[1];
            match vars.iter().find(|(k, _)| *k == name) {
                Some((_, v)) => (*v).to_string(),
                None => caps[0].to_string(),
            }
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_variables_returns_empty_for_clean_body() {
        let body =
            "Hi {{customer_name}}, your ticket {{ticket_id}} is being looked at by {{tech_name}}.";
        assert!(unknown_variables(body, CANNED_RESPONSE_VARIABLES).is_empty());
    }

    #[test]
    fn unknown_variables_flags_typos() {
        let body = "Hi {{custmer_name}}, ticket {{ticket_id}} from {{app_naem}}.";
        let mut flagged = unknown_variables(body, CANNED_RESPONSE_VARIABLES);
        flagged.sort();
        assert_eq!(flagged, vec!["app_naem", "custmer_name"]);
    }

    #[test]
    fn unknown_variables_dedups_repeated_unknowns() {
        let body = "{{foo}} {{foo}} {{foo}}";
        assert_eq!(
            unknown_variables(body, CANNED_RESPONSE_VARIABLES),
            vec!["foo"]
        );
    }

    #[test]
    fn unknown_variables_ignores_plain_braces() {
        let body = "Use { single } braces or {{ticket_id}} tokens, but not {{nope}}.";
        assert_eq!(
            unknown_variables(body, CANNED_RESPONSE_VARIABLES),
            vec!["nope"]
        );
    }

    #[test]
    fn unknown_variables_tolerates_whitespace_inside_braces() {
        let body = "Ticket #{{ ticket_id }} opened.";
        assert!(unknown_variables(body, CANNED_RESPONSE_VARIABLES).is_empty());
    }

    #[test]
    fn signature_allow_list_rejects_ticket_scoped_tokens() {
        // Ticket vars are valid for canned responses but not for
        // signatures (signatures are boilerplate, not templated
        // replies). The per-surface allow-list keeps that line crisp.
        let body = "Sent by {{tech_name}} re: ticket {{ticket_id}}.";
        let flagged = unknown_variables(body, SIGNATURE_VARIABLES);
        assert_eq!(flagged, vec!["ticket_id"]);
    }

    #[test]
    fn auto_ack_allow_list_rejects_tech_name() {
        // Auto-ack is system-authored, there's no agent. A template
        // with `{{tech_name}}` would substitute to the literal token
        // (no value bound), so reject at save time.
        let body = "Hi {{customer_name}}, your ticket {{ticket_id}} from {{tech_name}}.";
        let flagged = unknown_variables(body, AUTO_ACK_VARIABLES);
        assert_eq!(flagged, vec!["tech_name"]);
    }

    #[test]
    fn substitute_replaces_known_tokens() {
        let out = substitute(
            "Hi {{tech_name}} from {{app_name}}.",
            &[("tech_name", "Alex"), ("app_name", "Nosdesk")],
        );
        assert_eq!(out, "Hi Alex from Nosdesk.");
    }

    #[test]
    fn substitute_tolerates_whitespace_in_braces() {
        let out = substitute("Hi {{ tech_name }}!", &[("tech_name", "Alex")]);
        assert_eq!(out, "Hi Alex!");
    }

    #[test]
    fn substitute_leaves_unknown_tokens_in_place() {
        // Validation already rejects unknown tokens at save time;
        // if one reaches the renderer (e.g. data migrated from
        // before validation existed) we leave it visible rather
        // than silently dropping it.
        let out = substitute("Hi {{tech_name}}, {{mystery}}", &[("tech_name", "Alex")]);
        assert_eq!(out, "Hi Alex, {{mystery}}");
    }

    #[test]
    fn substitute_handles_repeated_tokens() {
        let out = substitute(
            "{{tech_name}} {{tech_name}} {{tech_name}}",
            &[("tech_name", "Sam")],
        );
        assert_eq!(out, "Sam Sam Sam");
    }

    #[test]
    fn first_name_returns_first_whitespace_token() {
        assert_eq!(first_name("Mary Jane Smith"), "Mary");
        assert_eq!(first_name("Alex"), "Alex");
        assert_eq!(first_name(""), "");
        assert_eq!(first_name("   "), "");
        assert_eq!(first_name("\t Jane\tDoe"), "Jane");
    }

    #[test]
    fn first_name_variables_are_on_relevant_allow_lists() {
        // Catches accidental list drift: customer_first_name belongs
        // anywhere customer_name does; same for tech_first_name vs
        // tech_name. A future contributor adding `customer_name` to
        // a new list would expect to add `customer_first_name` too.
        assert!(CANNED_RESPONSE_VARIABLES.contains(&"customer_first_name"));
        assert!(CANNED_RESPONSE_VARIABLES.contains(&"tech_first_name"));
        assert!(SIGNATURE_VARIABLES.contains(&"tech_first_name"));
        assert!(AUTO_ACK_VARIABLES.contains(&"customer_first_name"));
        // Symmetric: the auto-ack still doesn't carry tech vars.
        assert!(!AUTO_ACK_VARIABLES.contains(&"tech_first_name"));
        assert!(!AUTO_ACK_VARIABLES.contains(&"tech_name"));
    }
}
