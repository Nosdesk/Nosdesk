//! Chains of events: a rule's write is another rule's event, and the chain
//! keeps the root event's id as its correlation id. Each rule runs at most
//! once per chain on a ticket (decision D4); a rule the chain comes back to
//! is suppressed with a row, and a chain that would run more than the cap
//! of rules on one ticket is stopped by the loop guard.
#![allow(clippy::expect_used)]

use serde_json::json;

use backend::models::{
    RuleApplicationStatus as Status, RuleTriggerKind as Trigger, TicketPriority,
};
use backend::repository::rules::CHAIN_RULE_CAP;

use crate::rules_event_triggers::{leaf, marker, Harness, Spec};

/// A tags an urgent or high ticket; B raises a tagged ticket to high, which
/// A reads. From one priority change: A runs, B runs, and A, back again in
/// the same chain, is suppressed rather than running twice.
#[actix_rt::test]
async fn each_rule_runs_once_per_chain() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let tag = h.tag(a, "escalated");
    let rule_a = h.rule(
        a,
        Spec {
            trigger_config: json!({ "changed_fields_include": ["ticket.priority"] }),
            conditions: leaf("ticket.priority", "in", json!(["urgent", "high"])),
            ..Spec::live(
                Trigger::TicketUpdated,
                json!([{ "kind": "add_tags", "config": { "tag_ids": [tag] } }]),
            )
        },
    );
    let rule_b = h.rule(
        a,
        Spec {
            trigger_config: json!({ "changed_fields_include": ["ticket.tag_ids"] }),
            conditions: leaf("ticket.tag_ids", "has_any", json!([tag])),
            ..Spec::live(
                Trigger::TicketUpdated,
                json!([{ "kind": "set_priority", "config": { "priority": "high" } }]),
            )
        },
    );
    let ticket = h.ticket(a, "Printer offline");
    h.set_priority(a, ticket, TicketPriority::Urgent);
    h.drain().await;

    assert_eq!(
        h.statuses(a, rule_a),
        vec![Status::Succeeded, Status::SuppressedRecursionBudget]
    );
    assert_eq!(h.statuses(a, rule_b), vec![Status::Succeeded]);
    assert_eq!(h.tags(a, ticket), vec![tag]);
    assert_eq!(h.priority(a, ticket), TicketPriority::High);
    assert_eq!(h.rule_row(a, rule_a).fire_count, 1);

    // One chain: every row carries the root's id.
    let roots: Vec<_> = [rule_a, rule_b]
        .into_iter()
        .flat_map(|r| h.applications(a, r))
        .map(|row| row.correlation_id.expect("a chain id"))
        .collect();
    assert!(roots.windows(2).all(|w| w[0] == w[1]), "{roots:?}");

    // A new change is a new chain: A runs again.
    h.set_priority(a, ticket, TicketPriority::Urgent);
    h.drain().await;
    assert_eq!(
        h.statuses(a, rule_a)
            .iter()
            .filter(|s| **s == Status::Succeeded)
            .count(),
        2
    );
}

/// More rules than the cap on one event: the first `CHAIN_RULE_CAP` run,
/// and the next is stopped by the loop guard with a row of its own.
#[actix_rt::test]
async fn the_chain_cap_stops_runaway_rules() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let ids: Vec<i32> = (0..=CHAIN_RULE_CAP)
        .map(|i| {
            h.rule(
                a,
                Spec {
                    priority: i as i32,
                    ..Spec::live(Trigger::TicketCreated, marker())
                },
            )
        })
        .collect();
    h.ticket(a, "Printer offline");
    h.drain().await;

    let (last, ran) = ids.split_last().expect("rules");
    for id in ran {
        assert_eq!(h.statuses(a, *id), vec![Status::Succeeded], "rule {id}");
    }
    let guarded = h.applications(a, *last);
    assert_eq!(guarded.len(), 1);
    assert_eq!(guarded[0].status, Status::SuppressedLoopGuard);
    assert!(guarded[0]
        .failure_reason
        .as_deref()
        .is_some_and(|r| r.contains("50 rules")));
}
