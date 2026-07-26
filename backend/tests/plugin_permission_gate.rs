//! The plugin-owned data endpoints (storage, collections) authorize server-side
//! against the plugin's effective (consented) permission set via
//! `authorize_plugin_data_request` — not merely `is_active()`, and not the
//! frontend's advisory `hasPermission`. This exercises the gate directly against
//! seeded plugin rows: a consented permission passes, a non-consented one is
//! Forbidden, an inactive plugin is refused even for a consented permission, and
//! an unknown uuid is NotFound.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::handlers::plugins::{authorize_plugin_data_request, PluginGate};
use backend::models::{NewPlugin, Plugin, PluginState};
use backend::schema::plugins;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

mod common;

fn seed_plugin(
    conn: &mut backend::db::DbConnection,
    ws_id: i32,
    admin: Uuid,
    name: &str,
    consented: Option<Vec<&str>>,
    state: PluginState,
) -> Uuid {
    let actor = ActorContext::user(admin, None).with_workspace(ws_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        let new_plugin = NewPlugin {
            name: name.to_string(),
            display_name: name.to_string(),
            version: "1.0.0".to_string(),
            description: None,
            manifest: json!({ "name": name, "version": "1.0.0" }),
            state,
            trust_level: "verified".to_string(),
            installed_by: Some(admin),
            source: "provisioned".to_string(),
            signer_pubkey: None,
            signer_source: None,
            signature_metadata: None,
            icon_svg: None,
            consented_permissions: consented.map(|v| json!(v)),
            consented_at: None,
            consented_by: None,
        };
        let p: Plugin = diesel::insert_into(plugins::table)
            .values(&new_plugin)
            .get_result(c)?;
        Ok(p.uuid)
    })
    .expect("seed plugin")
}

/// Run the gate as a handler would: pinned to the workspace (RLS), returning the
/// authorization decision.
fn gate(
    conn: &mut backend::db::DbConnection,
    ws_id: i32,
    uuid: Uuid,
    needed: &str,
) -> Result<Plugin, PluginGate> {
    let actor = ActorContext::system("test:plugin_gate").with_workspace(ws_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        authorize_plugin_data_request(c, uuid, needed, "denied")
    })
    .expect("pinned gate")
}

#[test]
fn gate_enforces_consented_scope() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let ws = common::mint_workspace(&mut conn, "gate-ws", "Gate WS");
    let admin = common::insert_user(&mut conn, "Gate Admin").uuid;

    // Consented to storage:plugin + collection:read, NOT collection:write.
    let p = seed_plugin(
        &mut conn,
        ws,
        admin,
        "gate-granted",
        Some(vec!["storage:plugin", "collection:read"]),
        PluginState::Installed,
    );
    assert!(
        gate(&mut conn, ws, p, "storage:plugin").is_ok(),
        "a consented permission must pass the gate"
    );
    assert!(
        gate(&mut conn, ws, p, "collection:read").is_ok(),
        "a consented permission must pass the gate"
    );
    assert!(
        matches!(
            gate(&mut conn, ws, p, "collection:write"),
            Err(PluginGate::Forbidden(_))
        ),
        "a permission not in the consented set must be Forbidden"
    );

    // Disabled plugin: refused even for a consented permission.
    let disabled = seed_plugin(
        &mut conn,
        ws,
        admin,
        "gate-disabled",
        Some(vec!["storage:plugin"]),
        PluginState::Disabled,
    );
    assert!(
        matches!(
            gate(&mut conn, ws, disabled, "storage:plugin"),
            Err(PluginGate::Inactive)
        ),
        "a non-Installed plugin must be Inactive regardless of consent"
    );

    // Unknown uuid: NotFound (RLS-scoped).
    assert!(
        matches!(
            gate(&mut conn, ws, Uuid::now_v7(), "storage:plugin"),
            Err(PluginGate::NotFound)
        ),
        "an unknown plugin uuid must be NotFound"
    );
}
