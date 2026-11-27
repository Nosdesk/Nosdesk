//! Rules that email people. A notify step queues an email about the ticket
//! to its requester or to a member of the workspace, marked
//! `Auto-Submitted: auto-generated`, and posts nothing on the ticket. A
//! reply step on a rule that runs on its own posts as the agent it sends
//! as, names the rule in the comment's metadata and goes to the relay. An
//! automated email (an auto-reply, a mailing list) gets neither, and nor
//! does a ticket flagged as likely spam. The emails
//! are asserted on their outbound queue rows; nothing is sent.
#![allow(clippy::expect_used)]

use std::time::Duration;

use actix_web::http::StatusCode;
use actix_web::test as http_test;
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::models::{
    NewChannelMessage, OutboundEmail, RuleApplicationStatus as Status, RuleTriggerKind as Trigger,
};
use backend::repository::channels;
use backend::schema::{comments, outbound_emails, user_emails, users};

use crate::common::{self, WorkspaceSeed};
use crate::rules_event_triggers::{Harness, Spec};
use crate::rules_preview_match::{as_admin, json_of};

/// Give `user` a primary email address.
fn email(h: &Harness, user: Uuid, address: &str) {
    diesel::insert_into(user_emails::table)
        .values((
            user_emails::user_uuid.eq(user),
            user_emails::email.eq(address),
            user_emails::email_type.eq("personal"),
            user_emails::is_primary.eq(true),
            user_emails::is_verified.eq(true),
        ))
        .execute(&mut h.db.conn())
        .expect("insert email");
}

/// A new member of `ws` with `role`.
fn member(h: &Harness, ws: &WorkspaceSeed, name: &str, role: &str) -> Uuid {
    use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
    let user = common::insert_plain_user(&mut h.db.conn(), name);
    h.as_admin(ws, |c| {
        add_membership(
            c,
            ws.workspace_id,
            user,
            role,
            SeatWriteAuthority::ControlPlane,
        )
    });
    user
}

/// Soft-delete `user`, as deactivating their account does.
fn deactivate(h: &Harness, user: Uuid) {
    diesel::update(users::table.find(user))
        .set(users::deleted_at.eq(Some(chrono::Utc::now().naive_utc())))
        .execute(&mut h.db.conn())
        .expect("deactivate");
}

/// Every queued email about `ticket`, oldest first.
fn mail(h: &Harness, ws: &WorkspaceSeed, ticket: i32) -> Vec<OutboundEmail> {
    h.as_admin(ws, |c| {
        outbound_emails::table
            .filter(outbound_emails::ticket_id.eq(ticket))
            .order(outbound_emails::id)
            .load(c)
    })
}

fn comment_count(h: &Harness, ws: &WorkspaceSeed, ticket: i32) -> i64 {
    h.as_admin(ws, |c| {
        comments::table
            .filter(comments::ticket_id.eq(ticket))
            .count()
            .get_result(c)
    })
}

fn notify(recipient: Value) -> Value {
    let mut config = json!({
        "subject": "About {{ticket_title}}",
        "body": "Hi {{customer_first_name}}, we close quiet tickets after 30 days.",
    });
    for (k, v) in recipient.as_object().expect("an object") {
        config[k] = v.clone();
    }
    json!({ "kind": "notify", "config": config })
}

/// A notify step emails the requester and a named member: one queued email
/// each, auto-generated, about the ticket and filled in for it, and no
/// comment. The run records who it emailed.
#[actix_rt::test]
async fn notify_emails_the_requester_and_a_named_member() {
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    email(&h, a.admin_uuid, "lead@example.com");
    let rule = h.rule(
        a,
        Spec::live(
            Trigger::TicketCreated,
            json!([
                notify(json!({ "recipient": "requester" })),
                notify(json!({ "recipient": "user", "user_uuid": a.admin_uuid })),
            ]),
        ),
    );
    let ticket = h.ticket(a, "Printer & scanner");
    h.drain().await;

    let rows = h.applications(a, rule);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, Status::Succeeded);
    assert_eq!(
        rows[0].actions_taken,
        Some(json!([
            { "index": 1, "kind": "notify", "recipient": "requester" },
            { "index": 2, "kind": "notify", "recipient": "user", "user_uuid": a.admin_uuid },
        ]))
    );

    let sent = mail(&h, a, ticket);
    let to: Vec<&str> = sent.iter().map(|m| m.recipient.as_str()).collect();
    assert_eq!(to, vec!["requester@example.com", "lead@example.com"]);
    let number: i32 = h.as_admin(a, |c| {
        backend::repository::tickets::get_ticket_by_id(c, ticket).map(|t| t.number)
    });
    for m in &sent {
        assert_eq!(m.headers_json["Auto-Submitted"], "auto-generated");
        assert_eq!(m.comment_id, None);
        assert_eq!(m.status, "pending");
        // Plain text: the title goes in as typed.
        assert_eq!(m.subject, format!("[#{number}] About Printer & scanner"));
        assert!(
            m.body_text.contains("we close quiet tickets after 30 days"),
            "{}",
            m.body_text
        );
    }
    assert!(
        sent[0].body_text.starts_with("Hi a,"),
        "{}",
        sent[0].body_text
    );
    assert_eq!(comment_count(&h, a, ticket), 0, "an email, not a comment");
}

/// A requester with no email address has no one to tell: the step sends
/// nothing and says so, and the rest of the rule runs.
#[actix_rt::test]
async fn notify_without_an_address_sends_nothing_and_the_rule_runs_on() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let tag = h.tag(a, "warned");
    let rule = h.rule(
        a,
        Spec::live(
            Trigger::TicketCreated,
            json!([
                notify(json!({ "recipient": "requester" })),
                { "kind": "add_tags", "config": { "tag_ids": [tag] } },
            ]),
        ),
    );
    let ticket = h.ticket(a, "Printer offline");
    h.drain().await;

    let rows = h.applications(a, rule);
    assert_eq!(rows[0].status, Status::Succeeded);
    assert_eq!(
        rows[0].actions_taken.as_ref().expect("steps")[0],
        json!({ "index": 1, "kind": "notify", "recipient": "requester", "skipped": "no_email" })
    );
    assert!(mail(&h, a, ticket).is_empty());
    assert_eq!(h.tags(a, ticket), vec![tag]);
}

/// A reply that came from a mailing list gets neither a notify email nor a
/// public reply: both rules are held by the loop guard, with a row each.
#[actix_rt::test]
async fn an_automated_email_gets_no_notify_and_no_public_reply() {
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let any = json!({ "reply_kind": "any" });
    let notifies = h.rule(
        a,
        Spec {
            trigger_config: any.clone(),
            ..Spec::live(
                Trigger::TicketReplied,
                json!([notify(json!({ "recipient": "requester" }))]),
            )
        },
    );
    let replies = h.rule(
        a,
        Spec {
            trigger_config: any,
            ..Spec::live(
                Trigger::TicketReplied,
                json!([{ "kind": "reply", "config": { "visibility": "public", "body": "On it" } }]),
            )
        },
    );
    let ticket = h.ticket(a, "Weekly digest");
    let comment = h.reply(a, ticket, a.member_uuid);
    h.as_admin(a, |c| {
        let channel = channels::ensure_managed_channel(c, a.workspace_id)?;
        channels::record_message(
            c,
            NewChannelMessage {
                channel_id: channel.id,
                external_id: "<auto-1@example.com>".to_string(),
                direction: backend::models::CHANNEL_DIRECTION_INBOUND.to_string(),
                ticket_id: Some(ticket),
                comment_id: Some(comment),
                in_reply_to: None,
                from_address: Some("requester@example.com".to_string()),
                author_user_uuid: Some(a.member_uuid),
                raw_metadata: Some(json!({ "headers": [["Auto-Submitted", "auto-replied"]] })),
            },
        )
    });
    h.drain().await;

    assert_eq!(h.statuses(a, notifies), vec![Status::SuppressedLoopGuard]);
    assert_eq!(h.statuses(a, replies), vec![Status::SuppressedLoopGuard]);
    assert!(mail(&h, a, ticket).is_empty(), "no email");
    assert_eq!(comment_count(&h, a, ticket), 1, "no reply");
}

/// Flag `ticket` as likely spam, as the inbound pipeline does.
fn flag_spam(h: &Harness, ticket: i32) {
    use backend::schema::tickets;
    diesel::update(tickets::table.find(ticket))
        .set(tickets::spam_suspected.eq(true))
        .execute(&mut h.db.conn())
        .expect("flag as spam");
}

/// On a ticket flagged as likely spam, a rule that runs on its own holds
/// its steps that send email (a notify to the requester, a notify to staff,
/// a public reply) with a loop-guard row naming the reason, and runs the
/// rest: the tag goes on, an internal note is written. Another rule runs as
/// usual.
#[actix_rt::test]
async fn a_spam_ticket_gets_no_email_and_the_other_steps_run() {
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    email(&h, a.admin_uuid, "lead@example.com");
    let tag = h.tag(a, "acknowledged");
    let other_tag = h.tag(a, "seen");
    let rule = h.rule(
        a,
        Spec::live(
            Trigger::TicketCreated,
            json!([
                notify(json!({ "recipient": "requester" })),
                { "kind": "reply", "config": { "visibility": "public", "body": "Thanks" } },
                { "kind": "add_tags", "config": { "tag_ids": [tag] } },
                notify(json!({ "recipient": "user", "user_uuid": a.admin_uuid })),
                { "kind": "reply", "config": { "visibility": "internal", "body": "Check this one" } },
            ]),
        ),
    );
    let other = h.rule(
        a,
        Spec {
            priority: 200,
            ..Spec::live(
                Trigger::TicketCreated,
                json!([{ "kind": "add_tags", "config": { "tag_ids": [other_tag] } }]),
            )
        },
    );
    let ticket = h.ticket(a, "You have won a prize");
    flag_spam(&h, ticket);
    h.drain().await;

    let rows = h.applications(a, rule);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, Status::SuppressedLoopGuard);
    assert_eq!(
        rows[0].failure_reason.as_deref(),
        Some(backend::repository::rules::SPAM_HOLD_REASON)
    );
    assert_eq!(
        rows[0].actions_skipped,
        Some(json!([
            { "index": 1, "reason": "spam_suspected" },
            { "index": 2, "reason": "spam_suspected" },
            { "index": 4, "reason": "spam_suspected" },
        ]))
    );
    assert!(mail(&h, a, ticket).is_empty(), "no email");
    let notes: Vec<bool> = h.as_admin(a, |c| {
        comments::table
            .filter(comments::ticket_id.eq(ticket))
            .select(comments::is_internal)
            .load(c)
    });
    assert_eq!(notes, vec![true], "only the internal note");
    assert_eq!(h.statuses(a, other), vec![Status::Succeeded]);
    let mut expected = vec![tag, other_tag];
    expected.sort();
    assert_eq!(h.tags(a, ticket), expected);
}

/// An agent applying a rule to a ticket flagged as spam has looked at it:
/// the rule's email goes out.
#[actix_rt::test]
async fn an_agent_applying_a_rule_to_a_spam_ticket_sends_its_email() {
    use backend::repository::rules::{apply_manual, ApplyInput, ApplyOverrides};
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let rule = h.rule(
        a,
        Spec::live(
            Trigger::Manual,
            json!([notify(json!({ "recipient": "requester" }))]),
        ),
    );
    let ticket = h.ticket(a, "Not spam after all");
    flag_spam(&h, ticket);
    let mut conn = h.pool.get().expect("conn");
    let outcome = apply_manual(
        &mut conn,
        ApplyInput {
            rule_id: rule,
            ticket_id: ticket,
            overrides: ApplyOverrides::default(),
        },
        &crate::rules_event_triggers::admin(a),
        None,
    )
    .expect("the agent applies the rule");
    assert_eq!(outcome.application.status, Status::Succeeded);
    assert_eq!(mail(&h, a, ticket).len(), 1);
}

/// A reply step on an event rule posts as the agent it sends as, not the
/// rule's author, names the rule in the comment's metadata, and is handed
/// to the relay, which queues it for the requester.
#[actix_rt::test]
async fn an_event_reply_posts_as_its_agent_and_goes_to_the_relay() {
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let agent = member(&h, a, "Sam Agent", "agent");
    let rule = h.rule(
        a,
        Spec {
            trigger_config: json!({ "reply_kind": "customer" }),
            ..Spec::live(
                Trigger::TicketReplied,
                json!([{ "kind": "reply", "config": {
                    "visibility": "public",
                    "body": "Thanks {{customer_first_name}}, {{agent_first_name}} here.",
                    "send_as_uuid": agent,
                } }]),
            )
        },
    );
    let ticket = h.ticket(a, "Printer offline");
    h.reply(a, ticket, a.member_uuid);
    h.drain().await;

    assert_eq!(h.statuses(a, rule), vec![Status::Succeeded]);
    let (id, author, metadata, content): (i32, Uuid, Option<Value>, String) = h.as_admin(a, |c| {
        comments::table
            .filter(comments::ticket_id.eq(ticket))
            .order(comments::id.desc())
            .select((
                comments::id,
                comments::user_uuid,
                comments::channel_metadata,
                comments::content,
            ))
            .first(c)
    });
    assert_eq!(author, agent);
    assert!(content.contains("Sam here"), "{content}");
    assert_eq!(
        metadata,
        Some(json!({ "kind": "rule_reply", "visibility": "public", "rule_id": rule }))
    );

    // The relay queues the reply off the run, so wait for it.
    let mut relayed = Vec::new();
    for _ in 0..50 {
        relayed = mail(&h, a, ticket);
        if !relayed.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(relayed.len(), 1, "the reply was relayed");
    assert_eq!(relayed[0].comment_id, Some(id));
    assert_eq!(relayed[0].recipient, "requester@example.com");
}

/// An agent who can't work tickets any more fails an automated reply with a
/// `failed` row, and nothing is posted.
#[actix_rt::test]
async fn an_inactive_send_as_agent_fails_the_run() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let agent = member(&h, a, "Gone Agent", "agent");
    let rule = h.rule(
        a,
        Spec {
            trigger_config: json!({ "reply_kind": "customer" }),
            ..Spec::live(
                Trigger::TicketReplied,
                json!([{ "kind": "reply", "config": {
                    "visibility": "public", "body": "On it", "send_as_uuid": agent,
                } }]),
            )
        },
    );
    deactivate(&h, agent);
    let ticket = h.ticket(a, "Printer offline");
    h.reply(a, ticket, a.member_uuid);
    h.drain().await;

    let rows = h.applications(a, rule);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, Status::Failed);
    assert_eq!(
        rows[0].failure_reason.as_deref(),
        Some(
            "rule action [1] failed: the agent the reply sends as can no longer work tickets here"
        )
    );
    assert_eq!(comment_count(&h, a, ticket), 1, "nothing posted");
}

/// Saving refuses a reply on an event rule that sends as someone who can't
/// work tickets here (an inactive agent, a plain member) and a notify step
/// to someone who can't be emailed (not a member of the workspace), each
/// with its own code. A reply that names no one sends as its author, an
/// admin, and saves; so does a reply on a manual rule, which sends as
/// whoever applies it.
#[actix_rt::test]
async fn people_a_step_cannot_use_are_refused_at_save() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let (a, b) = (&h.seeded.a, &h.seeded.b);
    email(&h, b.admin_uuid, "elsewhere@example.com");
    let gone = member(&h, a, "Gone Agent", "agent");
    deactivate(&h, gone);

    let save =
        |trigger: &str, step: Value| {
            http_test::TestRequest::post().uri("/api/rules").set_json(json!({
            "name": "Answer replies",
            "trigger_kind": trigger,
            "trigger_config": {},
            "conditions": if trigger == "manual" { json!([]) } else {
                json!({ "kind": "and", "children": [
                    { "kind": "leaf", "field": "ticket.priority", "op": "eq", "value": "high" }
                ]})
            },
            "actions": [step],
        }))
        };
    let reply_as = |agent: Option<Uuid>| {
        json!({ "kind": "reply", "config": {
            "visibility": "public", "body": "On it", "send_as_uuid": agent,
        } })
    };

    for agent in [gone, a.member_uuid] {
        let (status, body) =
            json_of(as_admin(&h.pool, a, save("ticket_replied", reply_as(Some(agent)))).await)
                .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["code"], "RULE_SEND_AS_INELIGIBLE");
    }
    let (status, body) = json_of(
        as_admin(
            &h.pool,
            a,
            save(
                "ticket_created",
                notify(json!({ "recipient": "user", "user_uuid": b.admin_uuid })),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "RULE_NOTIFY_RECIPIENT_INVALID");

    for (trigger, step) in [
        ("ticket_replied", reply_as(None)),
        ("manual", reply_as(Some(gone))),
    ] {
        let (status, body) = json_of(as_admin(&h.pool, a, save(trigger, step)).await).await;
        assert_eq!(status, StatusCode::CREATED, "{trigger}: {body}");
    }
}

/// What a stranger typed as a ticket's title and their name.
const STRANGER_TITLE: &str = "Refund waiting at title-marker.example";
const STRANGER_NAME: &str = "Name Marker";

/// A ticket filed for `ws`'s member `via` a route nobody vouches for (an API
/// token, as a website relaying a form uses, or the request form without
/// confirmation): the title and the requester's name are whatever was typed.
fn stranger_ticket(h: &Harness, ws: &WorkspaceSeed, via: &str) -> i32 {
    diesel::update(users::table.find(ws.member_uuid))
        .set(users::name.eq(STRANGER_NAME))
        .execute(&mut h.db.conn())
        .expect("name the requester");
    h.as_admin(ws, |c| {
        let open = backend::repository::workflow_states::default_state(c)?;
        backend::repository::tickets::create_ticket(
            c,
            backend::models::NewTicket {
                title: STRANGER_TITLE.to_string(),
                workflow_state_id: open.id,
                requester_uuid: Some(ws.member_uuid),
                submitted_via: Some(via.to_string()),
                ..Default::default()
            },
        )
    })
    .id
}

fn names_the_ticket(m: &OutboundEmail) -> bool {
    [STRANGER_TITLE, STRANGER_NAME].iter().any(|marker| {
        m.subject.contains(marker)
            || m.body_text.contains(marker)
            || m.body_html.as_deref().is_some_and(|h| h.contains(marker))
    })
}

/// Wait for the relay, which queues a reply off the run, to queue `count`.
async fn mail_settled(
    h: &Harness,
    ws: &WorkspaceSeed,
    ticket: i32,
    count: usize,
) -> Vec<OutboundEmail> {
    let mut sent = Vec::new();
    for _ in 0..50 {
        sent = mail(h, ws, ticket);
        if sent.len() >= count {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    sent
}

/// On a ticket a stranger may have filed, a notify step and an automated
/// reply name the ticket by number and leave the requester's name out until
/// the requester proves their address; the reply's subject is "Re: [#N]".
/// After proof they name it again.
#[actix_rt::test]
async fn rule_mail_about_a_strangers_ticket_names_it_once_the_address_is_proven() {
    for via in [
        backend::models::SUBMITTED_VIA_API,
        backend::models::SUBMITTED_VIA_GUEST,
    ] {
        rule_mail_names_the_ticket_once_proven(via).await;
    }
}

async fn rule_mail_names_the_ticket_once_proven(via: &str) {
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let agent = member(&h, a, "Sam Agent", "agent");
    let steps = json!([
        notify(json!({ "recipient": "requester" })),
        { "kind": "reply", "config": {
            "visibility": "public",
            "body": "Hi {{customer_name}}, about {{ticket_title}}.",
            "send_as_uuid": agent,
        } },
    ]);
    h.rule(a, Spec::live(Trigger::TicketCreated, steps.clone()));
    h.rule(a, Spec::live(Trigger::TicketUpdated, steps));
    let ticket = stranger_ticket(&h, a, via);
    let number: i32 = h.as_admin(a, |c| {
        backend::repository::tickets::get_ticket_by_id(c, ticket).map(|t| t.number)
    });
    h.drain().await;

    let sent = mail_settled(&h, a, ticket, 2).await;
    assert_eq!(sent.len(), 2, "the notify and the relayed reply: {sent:?}");
    for m in &sent {
        assert_eq!(m.recipient, "requester@example.com");
        assert!(!names_the_ticket(m), "{m:?}");
    }
    assert_eq!(sent[0].subject, format!("[#{number}] About #{number}"));
    let reply = sent
        .iter()
        .find(|m| m.comment_id.is_some())
        .expect("the reply");
    assert_eq!(reply.subject, format!("Re: [#{number}]"));
    assert!(
        reply.body_text.contains(&format!("about #{number}")),
        "{}",
        reply.body_text
    );

    backend::repository::user_emails::mark_primary_verified(&mut h.db.conn(), &a.member_uuid)
        .expect("prove the address");
    h.set_priority(a, ticket, backend::models::TicketPriority::High);
    h.drain().await;
    let sent = mail_settled(&h, a, ticket, 4).await;
    assert!(sent.len() >= 4, "{sent:?}");
    for m in &sent[2..] {
        assert!(
            m.subject.contains(STRANGER_TITLE) || m.body_text.contains(STRANGER_TITLE),
            "named after proof: {m:?}"
        );
    }
}
