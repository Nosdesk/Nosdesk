//! The plugin-owned data endpoints (storage, collections) authorize server-side
//! against the plugin's effective (consented) permission set via
//! `authorize_plugin_data_request` — not merely `is_active()`, and not the
//! frontend's advisory `hasPermission`. This exercises the gate directly against
//! seeded plugin rows: a consented permission passes, a non-consented one is
//! Forbidden, an inactive plugin is refused even for a consented permission, and
//! an unknown uuid is NotFound. The caller must also be in the plugin's
//! audience (its widest component audience); anyone else gets NotFound.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::handlers::plugins::{authorize_plugin_data_request, PluginGate};
use backend::models::{NewPlugin, Plugin, PluginState};
use backend::schema::plugins;
use backend::services::plugins::access::PluginCaller;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

fn seed_plugin(
    conn: &mut backend::db::DbConnection,
    ws_id: i32,
    admin: Uuid,
    name: &str,
    consented: Option<Vec<&str>>,
    state: PluginState,
) -> Uuid {
    seed_plugin_shown_to(conn, ws_id, admin, name, consented, state, &["all"])
}

/// A plugin whose manifest parses and declares one component per audience.
fn manifest(name: &str, audiences: &[&str]) -> serde_json::Value {
    let components: serde_json::Map<String, serde_json::Value> = audiences
        .iter()
        .enumerate()
        .map(|(i, a)| {
            (
                format!("c{i}"),
                json!({ "slot": "ticket-sidebar", "entry": "c.js", "audience": a }),
            )
        })
        .collect();
    json!({
        "manifest_version": 1,
        "name": name,
        "displayName": name,
        "version": "1.0.0",
        "engines": { "nosdesk": ">=1.0.0", "plugin_api": "1" },
        "components": components,
    })
}

fn seed_plugin_shown_to(
    conn: &mut backend::db::DbConnection,
    ws_id: i32,
    admin: Uuid,
    name: &str,
    consented: Option<Vec<&str>>,
    state: PluginState,
    audiences: &[&str],
) -> Uuid {
    let actor = ActorContext::user(admin, None).with_workspace(ws_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        let new_plugin = NewPlugin {
            name: name.to_string(),
            display_name: name.to_string(),
            version: "1.0.0".to_string(),
            description: None,
            manifest: manifest(name, audiences),
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
    gate_as(conn, ws_id, uuid, PluginCaller::Staff, needed)
}

fn gate_as(
    conn: &mut backend::db::DbConnection,
    ws_id: i32,
    uuid: Uuid,
    caller: PluginCaller,
    needed: &str,
) -> Result<Plugin, PluginGate> {
    let actor = ActorContext::system("test:plugin_gate").with_workspace(ws_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        authorize_plugin_data_request(c, uuid, caller, needed, "denied")
    })
    .expect("pinned gate")
}

#[test]
fn gate_enforces_consented_scope() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ws = crate::common::mint_workspace(&mut conn, "gate-ws", "Gate WS");
    let admin = crate::common::insert_user(&mut conn, "Gate Admin").uuid;

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

#[test]
fn gate_admits_only_the_plugins_audience() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ws = crate::common::mint_workspace(&mut conn, "aud-ws", "Audience WS");
    let admin = crate::common::insert_user(&mut conn, "Audience Admin").uuid;
    let consent = || Some(vec!["storage:plugin"]);
    let installed = PluginState::Installed;
    let refused = |r: Result<Plugin, PluginGate>| matches!(r, Err(PluginGate::NotFound));

    let staff = seed_plugin_shown_to(
        &mut conn,
        ws,
        admin,
        "aud-staff",
        consent(),
        installed,
        &["staff", "admin"],
    );
    assert!(refused(gate_as(
        &mut conn,
        ws,
        staff,
        PluginCaller::Requester,
        "storage:plugin"
    )));
    assert!(gate_as(&mut conn, ws, staff, PluginCaller::Staff, "storage:plugin").is_ok());

    let admin_only = seed_plugin_shown_to(
        &mut conn,
        ws,
        admin,
        "aud-admin",
        consent(),
        installed,
        &["admin"],
    );
    assert!(refused(gate_as(
        &mut conn,
        ws,
        admin_only,
        PluginCaller::Staff,
        "storage:plugin"
    )));
    assert!(gate_as(
        &mut conn,
        ws,
        admin_only,
        PluginCaller::Admin,
        "storage:plugin"
    )
    .is_ok());

    let everyone = seed_plugin_shown_to(
        &mut conn,
        ws,
        admin,
        "aud-all",
        consent(),
        installed,
        &["staff", "all"],
    );
    assert!(gate_as(
        &mut conn,
        ws,
        everyone,
        PluginCaller::Requester,
        "storage:plugin"
    )
    .is_ok());

    let headless =
        seed_plugin_shown_to(&mut conn, ws, admin, "aud-none", consent(), installed, &[]);
    assert!(refused(gate_as(
        &mut conn,
        ws,
        headless,
        PluginCaller::Requester,
        "storage:plugin"
    )));
    assert!(gate_as(
        &mut conn,
        ws,
        headless,
        PluginCaller::Staff,
        "storage:plugin"
    )
    .is_ok());

    // A refused caller learns nothing about the plugin's state either.
    let disabled_staff = seed_plugin_shown_to(
        &mut conn,
        ws,
        admin,
        "aud-off",
        consent(),
        PluginState::Disabled,
        &["staff"],
    );
    assert!(refused(gate_as(
        &mut conn,
        ws,
        disabled_staff,
        PluginCaller::Requester,
        "storage:plugin"
    )));
}
