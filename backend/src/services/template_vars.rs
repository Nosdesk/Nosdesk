//! Context-driven template rendering for the rules engine's `reply`
//! action.
//!
//! Wraps `utils::template_variables` (which owns the `{{name}}`
//! regex and the flat `&str` substituter) and adds the typed
//! `TemplateContext` the rule lifecycle passes in. The context is
//! structured so Phase 2's event-triggered and Phase 3's
//! time-elapsed fires extend the binding set without changing the
//! function signature (decision 34): `event` and `reply` are
//! optional and stay `None` for Phase 1's manual apply path.
//!
//! Canned responses keep their existing path through
//! `utils::template_variables::substitute` directly; they don't need
//! the User / Ticket plumbing because the frontend builds the
//! variable map at insert time. The two paths share the same regex
//! and allow-list, so a body authored for a canned response renders
//! identically when it ends up inside a rule's reply action.

use crate::models::{Ticket, User};
use crate::utils::template_variables::{first_name, substitute};

/// Inputs to [`render`]. The lifecycle builds one of these and hands
/// it off; the renderer pulls variable bindings from it without
/// touching the database.
///
/// `event` and `reply` are present so Phase 2 can stamp event- and
/// reply-scoped bindings without breaking Phase 1 callers; the
/// renderer reads them but Phase 1 templates won't reference them so
/// the bindings stay unused for now.
pub struct TemplateContext<'a> {
    pub ticket: &'a Ticket,
    pub requester: Option<&'a User>,
    /// Who the message is from. A notify step's email has no agent when
    /// the rule's author is gone, and the agent tokens render empty.
    pub agent: Option<&'a User>,
    pub app_name: &'a str,
    pub event: Option<EventContext<'a>>,
    pub reply: Option<ReplyContext<'a>>,
}

/// Event-scoped bindings populated by Phase 2's event-triggered fire
/// path. Phase 1's manual apply path leaves the parent field as
/// `None`.
pub struct EventContext<'a> {
    pub kind: &'a str,
    pub changed_fields: &'a [String],
}

/// Reply-scoped bindings populated by Phase 2's `ticket_replied`
/// trigger. Phase 1's manual apply path leaves the parent field as
/// `None`.
pub struct ReplyContext<'a> {
    pub kind: &'a str,
    pub author_role: &'a str,
}

/// Render `template` against `ctx` for an HTML comment body.
/// Substituted values are HTML-escaped before insertion so an
/// untrusted binding (a requester whose IdP-synced display name
/// contains HTML metacharacters, say) cannot inject script /
/// markup into the rendered comment. The body template itself is
/// admin-authored and vetted at save, so it passes through
/// unescaped; admins can keep using deliberate `<br>` / `<b>`
/// markup.
///
/// Unknown tokens pass through verbatim per [`substitute`]'s
/// contract; the editor catches typos at save time via
/// [`utils::template_variables::unknown_variables`] with the
/// `RULE_REPLY_VARIABLES` allow-list.
pub fn render(template: &str, ctx: &TemplateContext<'_>) -> String {
    let escaped: Vec<(&str, String)> = bindings(ctx)
        .into_iter()
        .map(|(k, v)| (k, html_escape::encode_safe(&v).into_owned()))
        .collect();
    let pairs: Vec<(&str, &str)> = escaped.iter().map(|(k, v)| (*k, v.as_str())).collect();
    substitute(template, &pairs)
}

/// Render `template` against `ctx` as plain text, for an email's subject
/// line: the values go in as they are, since nothing reads the result as
/// HTML.
pub fn render_text(template: &str, ctx: &TemplateContext<'_>) -> String {
    let values = bindings(ctx);
    let pairs: Vec<(&str, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
    substitute(template, &pairs)
}

/// Every variable `ctx` binds, unescaped. The event and reply bindings
/// are there only for a run that has them.
fn bindings(ctx: &TemplateContext<'_>) -> Vec<(&'static str, String)> {
    // `{{ticket_id}}` is the number people know the ticket by.
    let agent_name = ctx.agent.map(|a| a.name.as_str()).unwrap_or("");
    let customer_name = ctx.requester.map(|r| r.name.as_str()).unwrap_or("");
    let mut pairs = vec![
        ("ticket_id", ctx.ticket.number.to_string()),
        ("ticket_title", ctx.ticket.title.clone()),
        ("customer_name", customer_name.to_string()),
        ("customer_first_name", first_name(customer_name)),
        ("tech_name", agent_name.to_string()),
        ("tech_first_name", first_name(agent_name)),
        ("agent_name", agent_name.to_string()),
        ("agent_first_name", first_name(agent_name)),
        ("app_name", ctx.app_name.to_string()),
    ];
    if let Some(event) = &ctx.event {
        pairs.push(("event_kind", event.kind.to_string()));
    }
    if let Some(reply) = &ctx.reply {
        pairs.push(("reply_kind", reply.kind.to_string()));
        pairs.push(("reply_author_role", reply.author_role.to_string()));
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TicketPriority;
    use chrono::Utc;
    use uuid::Uuid;

    fn stub_ticket(id: i32, title: &str) -> Ticket {
        let now = Utc::now().naive_utc();
        Ticket {
            id,
            uuid: uuid::Uuid::nil(),
            title: title.to_string(),
            priority: TicketPriority::Medium,
            requester_uuid: None,
            assignee_uuid: None,
            created_at: now,
            updated_at: now,
            created_by: None,
            closed_at: None,
            closed_by: None,
            category_id: None,
            submitted_via: None,
            guest_lookup_token: None,
            verification_state: None,
            origin_channel_id: None,
            triage_state: None,
            due_date: None,
            start_date: None,
            recurrence_rule: None,
            recurrence_template_id: None,
            resolution_notes: None,
            workspace_id: 1,
            workflow_state_id: 2,
            first_response_at: None,
            sla_response_target_at: None,
            sla_response_breached_at: None,
            sla_resolution_target_at: None,
            sla_resolution_breached_at: None,
            spam_suspected: false,
            sla_clock_started_at: None,
            sla_paused_at: None,
            sla_override: "auto".to_string(),
            approval_state: None,
            // Distinct from the id, so output that quotes the id fails.
            number: id + 1000,
            raised_by_staff: false,
            blank_at: None,
        }
    }

    fn stub_user(name: &str) -> User {
        let now = Utc::now().naive_utc();
        User {
            uuid: Uuid::nil(),
            name: name.to_string(),
            username: None,
            created_at: now,
            updated_at: now,
            password_changed_at: None,
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_enabled: false,
            feature_flag_overrides: serde_json::Value::Null,
            deleted_at: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            platform_role: "user".to_string(),
        }
    }

    #[test]
    fn renders_ticket_and_agent_bindings() {
        let ticket = stub_ticket(42, "Wi-Fi outage");
        let agent = stub_user("Kyle Phillips");
        let ctx = TemplateContext {
            ticket: &ticket,
            requester: None,
            agent: Some(&agent),
            app_name: "Nosdesk",
            event: None,
            reply: None,
        };
        let out = render(
            "Hi {{customer_name}}, ticket {{ticket_id}} ({{ticket_title}}) is being looked at by {{agent_name}}. Cheers, {{agent_first_name}} from {{app_name}}.",
            &ctx,
        );
        // Output is HTML-safe; ASCII-only inputs survive verbatim
        // through html_escape::encode_safe.
        assert_eq!(
            out,
            "Hi , ticket 1042 (Wi-Fi outage) is being looked at by Kyle Phillips. Cheers, Kyle from Nosdesk."
        );
    }

    #[test]
    fn html_metacharacters_in_substituted_values_are_escaped() {
        // The hostile-name case: a requester whose display name
        // carries an HTML payload (e.g. from an unsanitised OIDC
        // attribute). The body template's literal HTML
        // (`<strong>...</strong>`) is admin-authored and stays
        // intact; only the substituted value gets escaped.
        let ticket = stub_ticket(7, "Login broken");
        let agent = stub_user("Agent");
        let requester = stub_user("<img src=x onerror=alert(1)>");
        let ctx = TemplateContext {
            ticket: &ticket,
            requester: Some(&requester),
            agent: Some(&agent),
            app_name: "Nosdesk",
            event: None,
            reply: None,
        };
        let out = render(
            "<strong>Hi {{customer_name}}</strong>, we see ticket {{ticket_id}}.",
            &ctx,
        );
        assert!(out.contains("<strong>Hi"), "body template HTML survives");
        assert!(
            !out.contains("<img src=x"),
            "substituted value's raw HTML must be escaped, not rendered: {out}"
        );
        assert!(
            out.contains("&lt;img src=x"),
            "substituted value should appear in escaped form: {out}"
        );
    }

    #[test]
    fn renders_customer_bindings_when_requester_present() {
        let ticket = stub_ticket(7, "Login broken");
        let agent = stub_user("Alex Chen");
        let requester = stub_user("Jordan Lee");
        let ctx = TemplateContext {
            ticket: &ticket,
            requester: Some(&requester),
            agent: Some(&agent),
            app_name: "Nosdesk",
            event: None,
            reply: None,
        };
        let out = render(
            "Hi {{customer_first_name}}, we got your ticket {{ticket_id}}.",
            &ctx,
        );
        assert_eq!(out, "Hi Jordan, we got your ticket 1007.");
    }

    #[test]
    fn agent_and_tech_aliases_resolve_identically() {
        let ticket = stub_ticket(1, "T");
        let agent = stub_user("Mary Jane Smith");
        let ctx = TemplateContext {
            ticket: &ticket,
            requester: None,
            agent: Some(&agent),
            app_name: "Nosdesk",
            event: None,
            reply: None,
        };
        // `tech_*` and `agent_*` resolve to the same agent values so
        // a body authored for canned responses renders identically
        // inside a rule reply.
        let with_tech = render("{{tech_name}} {{tech_first_name}}", &ctx);
        let with_agent = render("{{agent_name}} {{agent_first_name}}", &ctx);
        assert_eq!(with_tech, "Mary Jane Smith Mary");
        assert_eq!(with_agent, with_tech);
    }

    #[test]
    fn unknown_tokens_pass_through_for_visibility() {
        // The renderer leaves unrecognised tokens intact so the typo
        // surfaces in preview rather than silently dropping. Save-
        // time validation in repository::rules catches them earlier.
        let ticket = stub_ticket(1, "T");
        let agent = stub_user("Agent");
        let ctx = TemplateContext {
            ticket: &ticket,
            requester: None,
            agent: Some(&agent),
            app_name: "Nosdesk",
            event: None,
            reply: None,
        };
        let out = render("{{custmer_name}} should not resolve", &ctx);
        assert_eq!(out, "{{custmer_name}} should not resolve");
    }

    /// A subject line is plain text: its values go in unescaped. With no
    /// agent, the agent tokens are empty.
    #[test]
    fn plain_text_keeps_values_as_they_are() {
        let ticket = stub_ticket(3, "Q&A <draft>");
        let requester = stub_user("Ann O'Brien");
        let ctx = TemplateContext {
            ticket: &ticket,
            requester: Some(&requester),
            agent: None,
            app_name: "Nosdesk",
            event: None,
            reply: None,
        };
        assert_eq!(
            render_text(
                "{{ticket_title}} for {{customer_first_name}}{{agent_name}}",
                &ctx
            ),
            "Q&A <draft> for Ann"
        );
        assert_eq!(render("{{ticket_title}}", &ctx), "Q&amp;A &lt;draft&gt;");
    }
}
