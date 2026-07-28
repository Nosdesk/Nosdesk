//! Cross-plugin isolation for collection rows.
//!
//! Row get/update/delete scope by the owning collection schema, so a plugin
//! cannot reach ANOTHER plugin's row by its UUID even within the same workspace.
//! RLS on `plugin_collection_rows` scopes only to the tenant (`workspace_id`),
//! not to the plugin, so without the `schema_id` filter a workspace member with
//! any `collection:read`/`write` plugin could read/overwrite/delete a sibling
//! plugin's row given its UUID. This exercises the repository scoping directly.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::models::{
    NewPlugin, NewPluginCollectionRow, NewPluginCollectionSchema, Plugin, PluginCollectionRow,
    PluginCollectionRowUpdate, PluginState,
};
use backend::repository::plugin_collections as repo;
use backend::schema::plugins;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

mod common;

fn seed_plugin(
    conn: &mut backend::db::DbConnection,
    ws_id: i32,
    admin: Uuid,
    name: &str,
) -> Plugin {
    let actor = ActorContext::user(admin, None).with_workspace(ws_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        let new_plugin = NewPlugin {
            name: name.to_string(),
            display_name: name.to_string(),
            version: "1.0.0".to_string(),
            description: None,
            manifest: json!({ "name": name, "version": "1.0.0" }),
            state: PluginState::Installed,
            trust_level: "verified".to_string(),
            installed_by: Some(admin),
            source: "provisioned".to_string(),
            signer_pubkey: None,
            signer_source: None,
            signature_metadata: None,
            icon_svg: None,
            consented_permissions: Some(json!(["collection:read", "collection:write"])),
            consented_at: None,
            consented_by: None,
        };
        diesel::insert_into(plugins::table)
            .values(&new_plugin)
            .get_result::<Plugin>(c)
    })
    .expect("seed plugin")
}

#[test]
fn collection_rows_are_scoped_to_their_plugin_schema() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let ws = common::mint_workspace(&mut conn, "row-scope-ws", "Row Scope WS");
    let admin = common::insert_user(&mut conn, "Row Admin").uuid;

    let plugin_a = seed_plugin(&mut conn, ws, admin, "plugin-a");
    let plugin_b = seed_plugin(&mut conn, ws, admin, "plugin-b");

    let actor = ActorContext::user(admin, None).with_workspace(ws);

    // Each plugin owns a collection named "notes"; only B has a row.
    let (schema_a_id, schema_b_id, row_b) =
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            let schema_a = repo::create_schema(
                c,
                NewPluginCollectionSchema {
                    plugin_id: plugin_a.id,
                    collection_name: "notes".into(),
                    schema: json!({ "fields": [] }),
                    version: 1,
                },
            )?;
            let schema_b = repo::create_schema(
                c,
                NewPluginCollectionSchema {
                    plugin_id: plugin_b.id,
                    collection_name: "notes".into(),
                    schema: json!({ "fields": [] }),
                    version: 1,
                },
            )?;
            let row_b = repo::create_row(
                c,
                NewPluginCollectionRow {
                    plugin_id: plugin_b.id,
                    schema_id: schema_b.id,
                    data: json!({ "secret": "b" }),
                    created_by: Some(admin),
                },
            )?;
            Ok((schema_a.id, schema_b.id, row_b))
        })
        .expect("seed collections");

    // Owner (schema B) reads its own row.
    let owner_read =
        with_actor_context::<PluginCollectionRow, diesel::result::Error>(&mut conn, &actor, |c| {
            repo::get_row_by_uuid(c, schema_b_id, row_b.uuid)
        });
    assert!(owner_read.is_ok(), "owner must read its own row");

    // Cross-plugin READ via schema A is NotFound.
    let cross_read =
        with_actor_context::<PluginCollectionRow, diesel::result::Error>(&mut conn, &actor, |c| {
            repo::get_row_by_uuid(c, schema_a_id, row_b.uuid)
        });
    assert!(
        matches!(cross_read, Err(diesel::result::Error::NotFound)),
        "a plugin must not read another plugin's row by uuid"
    );

    // Cross-plugin UPDATE via schema A touches nothing.
    let cross_update =
        with_actor_context::<PluginCollectionRow, diesel::result::Error>(&mut conn, &actor, |c| {
            repo::update_row(
                c,
                schema_a_id,
                row_b.uuid,
                PluginCollectionRowUpdate {
                    data: Some(json!({ "secret": "pwned" })),
                },
            )
        });
    assert!(
        matches!(cross_update, Err(diesel::result::Error::NotFound)),
        "a plugin must not update another plugin's row"
    );

    // Cross-plugin DELETE via schema A removes nothing.
    let cross_delete = with_actor_context::<usize, diesel::result::Error>(&mut conn, &actor, |c| {
        repo::delete_row(c, schema_a_id, row_b.uuid)
    })
    .expect("delete runs");
    assert_eq!(
        cross_delete, 0,
        "a plugin must not delete another plugin's row"
    );

    // B's row is intact and its owner can delete it.
    let still_there =
        with_actor_context::<PluginCollectionRow, diesel::result::Error>(&mut conn, &actor, |c| {
            repo::get_row_by_uuid(c, schema_b_id, row_b.uuid)
        });
    assert!(
        still_there.is_ok(),
        "the row must survive cross-plugin tampering attempts"
    );
    assert_eq!(
        still_there.expect("row").data,
        json!({ "secret": "b" }),
        "the row data must be unchanged"
    );

    let owner_delete = with_actor_context::<usize, diesel::result::Error>(&mut conn, &actor, |c| {
        repo::delete_row(c, schema_b_id, row_b.uuid)
    })
    .expect("owner delete runs");
    assert_eq!(owner_delete, 1, "the owner must delete its own row");
}
