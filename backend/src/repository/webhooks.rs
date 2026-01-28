//! Webhook Repository
//!
//! Provides database operations for webhooks and webhook deliveries.

use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    NewWebhook, NewWebhookDelivery, Webhook, WebhookDelivery, WebhookDeliveryUpdate, WebhookUpdate,
};
use crate::schema::{webhook_deliveries, webhooks};

/// List all webhooks
pub fn list_all_webhooks(conn: &mut DbConnection) -> Result<Vec<Webhook>, diesel::result::Error> {
    webhooks::table
        .order(webhooks::created_at.desc())
        .load::<Webhook>(conn)
}

/// Get webhooks that are enabled and subscribed to a specific event type
pub fn get_webhooks_for_event(
    conn: &mut DbConnection,
    event_type: &str,
) -> Result<Vec<Webhook>, String> {
    webhooks::table
        .filter(webhooks::enabled.eq(true))
        .filter(webhooks::events.contains(vec![Some(event_type.to_string())]))
        .load::<Webhook>(conn)
        .map_err(|e| format!("Database error: {e}"))
}

/// Create a new webhook
pub fn create_webhook(
    conn: &mut DbConnection,
    name: String,
    url: String,
    secret: String,
    events: Vec<String>,
    headers: Option<serde_json::Value>,
    created_by: Option<Uuid>,
) -> Result<Webhook, diesel::result::Error> {
    let new_webhook = NewWebhook {
        name,
        url,
        secret,
        events: events.into_iter().map(Some).collect(),
        enabled: true,
        headers,
        created_by,
    };

    diesel::insert_into(webhooks::table)
        .values(&new_webhook)
        .get_result(conn)
}

/// Get a webhook by ID
pub fn get_webhook_by_id(
    conn: &mut DbConnection,
    webhook_id: i32,
) -> Result<Webhook, String> {
    webhooks::table
        .filter(webhooks::id.eq(webhook_id))
        .first::<Webhook>(conn)
        .map_err(|e| format!("Database error: {e}"))
}

/// Get a webhook by UUID
pub fn get_webhook_by_uuid(
    conn: &mut DbConnection,
    webhook_uuid: Uuid,
) -> Result<Webhook, diesel::result::Error> {
    webhooks::table
        .filter(webhooks::uuid.eq(webhook_uuid))
        .first::<Webhook>(conn)
}

/// Update a webhook by ID
pub fn update_webhook(
    conn: &mut DbConnection,
    webhook_id: i32,
    update: WebhookUpdate,
) -> Result<Webhook, String> {
    diesel::update(webhooks::table.filter(webhooks::id.eq(webhook_id)))
        .set(&update)
        .get_result(conn)
        .map_err(|e| format!("Database error: {e}"))
}

/// Update a webhook by UUID
pub fn update_webhook_by_uuid(
    conn: &mut DbConnection,
    webhook_uuid: Uuid,
    update: WebhookUpdate,
) -> Result<Webhook, diesel::result::Error> {
    diesel::update(webhooks::table.filter(webhooks::uuid.eq(webhook_uuid)))
        .set(&update)
        .get_result(conn)
}

/// Delete a webhook by UUID
pub fn delete_webhook_by_uuid(
    conn: &mut DbConnection,
    webhook_uuid: Uuid,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(webhooks::table.filter(webhooks::uuid.eq(webhook_uuid))).execute(conn)
}

// ===== WEBHOOK DELIVERIES =====

/// Create a new webhook delivery record
pub fn create_delivery(
    conn: &mut DbConnection,
    new_delivery: NewWebhookDelivery,
) -> Result<WebhookDelivery, String> {
    diesel::insert_into(webhook_deliveries::table)
        .values(&new_delivery)
        .get_result(conn)
        .map_err(|e| format!("Database error: {e}"))
}

/// Update a webhook delivery
pub fn update_delivery(
    conn: &mut DbConnection,
    delivery_id: i32,
    update: WebhookDeliveryUpdate,
) -> Result<WebhookDelivery, String> {
    diesel::update(webhook_deliveries::table.filter(webhook_deliveries::id.eq(delivery_id)))
        .set(&update)
        .get_result(conn)
        .map_err(|e| format!("Database error: {e}"))
}

/// Get deliveries for a specific webhook (paginated)
pub fn get_deliveries_for_webhook(
    conn: &mut DbConnection,
    webhook_id: i32,
    limit: i64,
    offset: i64,
) -> Result<Vec<WebhookDelivery>, diesel::result::Error> {
    webhook_deliveries::table
        .filter(webhook_deliveries::webhook_id.eq(webhook_id))
        .order(webhook_deliveries::created_at.desc())
        .limit(limit)
        .offset(offset)
        .load::<WebhookDelivery>(conn)
}

/// Get pending retries (deliveries with next_retry_at in the past and not yet delivered)
pub fn get_pending_retries(conn: &mut DbConnection) -> Result<Vec<WebhookDelivery>, String> {
    let now = Utc::now().naive_utc();

    webhook_deliveries::table
        .filter(webhook_deliveries::delivered_at.is_null())
        .filter(webhook_deliveries::next_retry_at.is_not_null())
        .filter(webhook_deliveries::next_retry_at.le(now))
        .order(webhook_deliveries::next_retry_at.asc())
        .limit(100) // Process up to 100 retries at a time
        .load::<WebhookDelivery>(conn)
        .map_err(|e| format!("Database error: {e}"))
}

/// Delete old deliveries (for cleanup)
#[allow(dead_code)]
pub fn delete_old_deliveries(
    conn: &mut DbConnection,
    days_old: i64,
) -> Result<usize, diesel::result::Error> {
    let cutoff = Utc::now().naive_utc() - chrono::Duration::days(days_old);

    diesel::delete(webhook_deliveries::table.filter(webhook_deliveries::created_at.lt(cutoff)))
        .execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;

    #[test]
    fn create_and_list_webhook() {
        let mut conn = setup_test_connection();

        let wh = create_webhook(
            &mut conn,
            "Test Hook".into(),
            "https://example.com/hook".into(),
            "secret123".into(),
            vec!["ticket.created".into()],
            None,
            None,
        )
        .unwrap();

        assert_eq!(wh.name, "Test Hook");

        let all = list_all_webhooks(&mut conn).unwrap();
        assert!(all.iter().any(|w| w.uuid == wh.uuid));
    }

    #[test]
    fn get_webhook_by_uuid_test() {
        let mut conn = setup_test_connection();

        let wh = create_webhook(
            &mut conn,
            "UUID Hook".into(),
            "https://example.com/uuid".into(),
            "s".into(),
            vec![],
            None,
            None,
        )
        .unwrap();

        let fetched = get_webhook_by_uuid(&mut conn, wh.uuid).unwrap();
        assert_eq!(fetched.name, "UUID Hook");
    }

    #[test]
    fn update_webhook_by_uuid_test() {
        let mut conn = setup_test_connection();

        let wh = create_webhook(
            &mut conn,
            "Old Name".into(),
            "https://example.com".into(),
            "s".into(),
            vec![],
            None,
            None,
        )
        .unwrap();

        let update = WebhookUpdate {
            name: Some("New Name".to_string()),
            ..Default::default()
        };
        let updated = update_webhook_by_uuid(&mut conn, wh.uuid, update).unwrap();
        assert_eq!(updated.name, "New Name");
    }

    #[test]
    fn delete_webhook_by_uuid_test() {
        let mut conn = setup_test_connection();

        let wh = create_webhook(
            &mut conn,
            "Delete Me".into(),
            "https://example.com".into(),
            "s".into(),
            vec![],
            None,
            None,
        )
        .unwrap();

        let rows = delete_webhook_by_uuid(&mut conn, wh.uuid).unwrap();
        assert_eq!(rows, 1);
        assert!(get_webhook_by_uuid(&mut conn, wh.uuid).is_err());
    }

    #[test]
    fn get_webhooks_for_event_test() {
        let mut conn = setup_test_connection();

        create_webhook(
            &mut conn,
            "Event Hook".into(),
            "https://example.com/event".into(),
            "s".into(),
            vec!["ticket.created".into()],
            None,
            None,
        )
        .unwrap();

        let hooks = get_webhooks_for_event(&mut conn, "ticket.created").unwrap();
        assert!(!hooks.is_empty());
        assert!(hooks.iter().any(|w| w.name == "Event Hook"));
    }
}
