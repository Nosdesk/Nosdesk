//! Dashboard breakdowns name what they count. A category or assignee
//! bucket carries the category's or person's name beside its key, so the
//! chart shows "Hardware" and "Dana Agent" rather than an id and a uuid.
//! Priority and the "none" / "unassigned" buckets carry no name: the
//! frontend labels those itself, in the viewer's language.

#![allow(clippy::expect_used)]

use chrono::{Duration, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};

use backend::models::{NewTicket, NewTicketCategory};
use backend::repository::analytics::{self, BreakdownGroupBy, BreakdownQuery};

const WS: i32 = 1;

fn buckets(conn: &mut backend::db::DbConnection, group_by: BreakdownGroupBy) -> Vec<Value> {
    let now = Utc::now();
    let result = analytics::breakdown(
        conn,
        BreakdownQuery {
            group_by,
            from: now - Duration::days(1),
            to: now + Duration::days(1),
            top_n: 10,
        },
    )
    .expect("breakdown");
    serde_json::to_value(result).expect("json")["buckets"]
        .as_array()
        .expect("buckets")
        .clone()
}

#[test]
fn category_and_assignee_buckets_carry_names() {
    use backend::schema::{ticket_categories, tickets, workflow_states};

    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let state: i32 = workflow_states::table
        .filter(workflow_states::workspace_id.eq(WS))
        .filter(workflow_states::is_default.eq(true))
        .select(workflow_states::id)
        .first(&mut conn)
        .expect("default state");
    let category: i32 = diesel::insert_into(ticket_categories::table)
        .values(&NewTicketCategory {
            name: "Hardware".to_string(),
            description: None,
            color: None,
            icon: None,
            display_order: 0,
            is_active: true,
            created_by: None,
            requester_visible: false,
            approval_required: false,
            approval_rule: "any".to_string(),
            approval_by_manager: false,
        })
        .returning(ticket_categories::id)
        .get_result(&mut conn)
        .expect("category");
    let agent = crate::common::insert_plain_user(&mut conn, "Dana Agent");
    for (category_id, assignee) in [
        (Some(category), Some(agent)),
        (Some(category), None),
        (None, Some(agent)),
    ] {
        diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Printer jam".to_string(),
                workflow_state_id: state,
                category_id,
                assignee_uuid: assignee,
                ..Default::default()
            })
            .execute(&mut conn)
            .expect("ticket");
    }

    let by_category = buckets(&mut conn, BreakdownGroupBy::Category);
    assert!(
        by_category
            .contains(&json!({ "key": category.to_string(), "label": "Hardware", "value": 2 })),
        "{by_category:?}"
    );
    assert!(
        by_category.contains(&json!({ "key": "none", "label": null, "value": 1 })),
        "{by_category:?}"
    );

    let by_assignee = buckets(&mut conn, BreakdownGroupBy::Assignee);
    assert!(
        by_assignee
            .contains(&json!({ "key": agent.to_string(), "label": "Dana Agent", "value": 2 })),
        "{by_assignee:?}"
    );
    assert!(
        by_assignee.contains(&json!({ "key": "unassigned", "label": null, "value": 1 })),
        "{by_assignee:?}"
    );

    // Priority keys are labelled by the frontend.
    let by_priority = buckets(&mut conn, BreakdownGroupBy::Priority);
    assert!(
        by_priority.iter().all(|b| b["label"].is_null()),
        "{by_priority:?}"
    );
}
