//! Links to other tickets in an outbound reply.
//!
//! The editor stores a link to another ticket as an empty
//! `<span data-ticket-link="true" data-ticket-id="…">` that the app draws as
//! a card, so in an email it read as nothing at all: "see " and a gap. Before
//! a reply goes out, each link gets its ticket's number (`#208`) as text, for
//! the HTML part and, through it, the plain-text part. The number is looked
//! up by id at send time, so a link written as Markdown (`#208`, already
//! filled) also carries the ticket's current number.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::db::DbConnection;

/// A ticket link element, opening tag through its closing tag. The editor
/// writes these empty and the Markdown path with `#n` inside; neither nests
/// another span.
static TICKET_LINK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?s)(<span\b[^>]*\bdata-ticket-link="true"[^>]*>).*?</span>"#)
        .expect("valid regex")
});

static TICKET_ID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"\bdata-ticket-id="(\d+)""#).expect("valid regex"));

fn linked_id(opening_tag: &str) -> Option<i32> {
    TICKET_ID_RE
        .captures(opening_tag)
        .and_then(|c| c[1].parse().ok())
}

/// The ids of the tickets `html` links to.
pub fn linked_ticket_ids(html: &str) -> Vec<i32> {
    let mut ids: Vec<i32> = TICKET_LINK_RE
        .captures_iter(html)
        .filter_map(|c| linked_id(&c[1]))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// `html` with each ticket link reading `#<number>`, by id. A link to a
/// ticket not in `numbers` (deleted, or not in this workspace) is left as
/// it was.
pub fn fill_ticket_links(html: &str, numbers: &HashMap<i32, i32>) -> String {
    TICKET_LINK_RE
        .replace_all(html, |c: &regex::Captures| {
            let opening = &c[1];
            match linked_id(opening).and_then(|id| numbers.get(&id)) {
                Some(number) => format!("{opening}#{number}</span>"),
                None => c[0].to_string(),
            }
        })
        .into_owned()
}

/// `html` with its ticket links filled from the tickets in `workspace_id`.
/// On a lookup error the links go out as they were, rather than the reply
/// not going out.
pub fn with_ticket_numbers(conn: &mut DbConnection, workspace_id: i32, html: &str) -> String {
    let ids = linked_ticket_ids(html);
    if ids.is_empty() {
        return html.to_string();
    }
    match crate::repository::tickets::numbers_of(conn, workspace_id, &ids) {
        Ok(numbers) => fill_ticket_links(html, &numbers),
        Err(e) => {
            tracing::warn!(error = %e, "reply ticket links: number lookup failed");
            html.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDITOR_LINK: &str =
        r#"<span data-ticket-link="true" data-ticket-id="211" data-href="/tickets/id/211"></span>"#;

    #[test]
    fn an_editor_link_reads_as_the_ticket_number() {
        let html = format!("<p>Same fault as {EDITOR_LINK}, fixed there.</p>");
        let numbers = HashMap::from([(211, 208)]);
        let out = fill_ticket_links(&html, &numbers);
        assert_eq!(
            out,
            r#"<p>Same fault as <span data-ticket-link="true" data-ticket-id="211" data-href="/tickets/id/211">#208</span>, fixed there.</p>"#
        );
        assert!(
            crate::utils::content::html_to_plaintext(&out).contains("Same fault as #208, fixed")
        );
    }

    #[test]
    fn a_filled_link_takes_the_current_number_and_unknown_links_stay() {
        let html = r#"<p><span data-ticket-id="5" data-ticket-link="true">#4</span> and <span data-ticket-link="true" data-ticket-id="9"></span> and <span class="x">plain</span></p>"#;
        let out = fill_ticket_links(html, &HashMap::from([(5, 7)]));
        assert_eq!(
            out,
            r#"<p><span data-ticket-id="5" data-ticket-link="true">#7</span> and <span data-ticket-link="true" data-ticket-id="9"></span> and <span class="x">plain</span></p>"#
        );
    }

    #[test]
    fn linked_ids_are_listed_once() {
        let html = format!("{EDITOR_LINK} {EDITOR_LINK} <span data-ticket-link=\"true\" data-ticket-id=\"3\"></span>");
        assert_eq!(linked_ticket_ids(&html), vec![3, 211]);
        assert!(linked_ticket_ids("<p>no links</p>").is_empty());
    }

    #[test]
    fn numbers_come_from_the_ticket_workspace() {
        use crate::test_helpers::{setup_test_connection, TestFixtures};
        let mut conn = setup_test_connection();
        let linked = TestFixtures::create_ticket(&mut conn, "Linked", None, None);
        let html = format!(
            r#"<p>See <span data-ticket-link="true" data-ticket-id="{}"></span>.</p>"#,
            linked.id
        );
        let out = with_ticket_numbers(&mut conn, linked.workspace_id, &html);
        assert!(
            out.contains(&format!(">#{}</span>", linked.number)),
            "got: {out}"
        );
        // The same link read for another workspace stays as it was.
        assert_eq!(
            with_ticket_numbers(&mut conn, linked.workspace_id + 1_000_000, &html),
            html
        );
    }
}
