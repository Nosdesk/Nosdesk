//! Directory sync stays in the workspace it runs for. The Microsoft sync runs
//! elevated past row security, pinned to its workspace (the LDAP sync runs
//! pinned without elevating), so the lookups a sync makes by directory id name
//! that workspace themselves: two workspaces syncing the same directory each
//! get their own groups, devices, delta tokens and contact rows.

use diesel::prelude::*;
use diesel::result::Error;
use serde_json::json;

use backend::db::DbConnection;
use backend::models::{DirectoryAddress, DirectoryContact, NewAsset};
use backend::repository::{assets, groups, sync_history, user_contact};
use backend::schema::{groups as groups_table, user_addresses, user_phone_numbers, user_profiles};
use backend::sync::actor::ActorContext;
use backend::sync::session::{elevate_session_role, reset_session_role};

use crate::common::{self, TestPool};

const REF: &str = "test:directory_sync_workspace_scope";

/// Run `f` the way directory sync does: elevated for the session and pinned to
/// `workspace_id`, or pinned to nothing.
fn as_sync<T>(
    pool: &TestPool,
    workspace_id: Option<i32>,
    f: impl FnOnce(&mut DbConnection) -> T,
) -> T {
    let mut conn = pool.get().expect("conn");
    let actor = match workspace_id {
        Some(ws) => ActorContext::system(REF).with_workspace(ws),
        None => ActorContext::system(REF),
    };
    elevate_session_role(&mut conn, &actor).expect("elevate");
    if workspace_id.is_none() {
        // The test pool pins a workspace on checkout.
        diesel::sql_query("SELECT set_config('app.workspace_id', '', false)")
            .execute(&mut conn)
            .expect("unpin");
    }
    let out = f(&mut conn);
    reset_session_role(&mut conn);
    out
}

fn device(name: &str, entra_id: &str, azure_id: &str) -> NewAsset {
    NewAsset {
        name: name.to_string(),
        serial_number: None,
        manufacturer: None,
        model: None,
        location: None,
        notes: None,
        primary_user_uuid: None,
        purchase_date: None,
        asset_tag: None,
        kind: "device".to_string(),
        attributes: json!({ "entra_device_id": entra_id, "microsoft_device_id": azure_id }),
        quantity: None,
        unit: None,
        external_sync_source: Some("microsoft".to_string()),
        low_stock_threshold: None,
    }
}

#[test]
fn a_directory_group_is_synced_into_each_workspace_separately() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);

    let upsert = |ws: i32, name: &str| {
        as_sync(&pool, Some(ws), |c| {
            groups::upsert_external_group(
                c,
                "directory-group-1",
                "ldap",
                name,
                None,
                Some("security"),
                false,
                true,
            )
            .expect("upsert group")
        })
    };
    let (a_group, created) = upsert(a, "Staff");
    assert!(created);
    let (b_group, created) = upsert(b, "All staff");
    assert!(created, "B gets its own group for the same directory id");
    assert_eq!((a_group.workspace_id, b_group.workspace_id), (a, b));

    // B's next sync sees no groups, so it marks only its own stale.
    as_sync(&pool, Some(b), |c| {
        groups::mark_groups_not_synced(c, "ldap", &[]).expect("mark stale")
    });

    let state = |id: i32| {
        as_sync(&pool, Some(a), |c| {
            groups_table::table
                .find(id)
                .select((groups_table::name, groups_table::sync_enabled))
                .first::<(String, bool)>(c)
                .expect("load group")
        })
    };
    assert_eq!(state(a_group.id), ("Staff".to_string(), true));
    assert_eq!(state(b_group.id), ("All staff".to_string(), false));
}

#[test]
fn device_lookups_by_directory_id_stay_in_the_workspace() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);

    let laptop = as_sync(&pool, Some(a), |c| {
        assets::create_device(c, device("LAPTOP-0042", "entra-1", "azure-1")).expect("device")
    });

    as_sync(&pool, Some(b), |c| {
        assert!(matches!(
            assets::get_device_by_entra_id(c, "entra-1"),
            Err(Error::NotFound)
        ));
        assert!(matches!(
            assets::get_device_by_microsoft_id(c, "azure-1"),
            Err(Error::NotFound)
        ));
        assert!(assets::get_devices_by_entra_ids(c, &["entra-1"])
            .expect("lookup")
            .is_empty());
        assert!(assets::get_devices_by_entra_ids_full(c, &["entra-1"])
            .expect("lookup")
            .is_empty());
        assert!(assets::get_devices_by_microsoft_ids_full(c, &["azure-1"])
            .expect("lookup")
            .is_empty());
    });

    as_sync(&pool, Some(a), |c| {
        assert_eq!(
            assets::get_device_by_entra_id(c, "entra-1")
                .expect("A's device")
                .id,
            laptop.id
        );
        assert_eq!(
            assets::get_devices_by_entra_ids(c, &["entra-1"]).expect("lookup"),
            vec![("entra-1".to_string(), laptop.id)]
        );
    });

    as_sync(&pool, None, |c| {
        assert!(
            assets::get_devices_by_entra_ids(c, &["entra-1"])
                .expect("lookup")
                .is_empty(),
            "an unpinned connection finds nothing"
        );
    });
}

#[test]
fn each_workspace_keeps_its_own_delta_token() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);

    as_sync(&pool, Some(a), |c| {
        sync_history::upsert_delta_token(c, "microsoft", "users", "https://graph.test/a")
            .expect("A's token")
    });
    as_sync(&pool, Some(b), |c| {
        assert!(matches!(
            sync_history::get_delta_token(c, "microsoft", "users"),
            Err(Error::NotFound)
        ));
        sync_history::upsert_delta_token(c, "microsoft", "users", "https://graph.test/b")
            .expect("B's token");
        assert_eq!(
            sync_history::delete_delta_token(c, "microsoft", "users").expect("delete"),
            1
        );
    });

    let link = as_sync(&pool, Some(a), |c| {
        sync_history::get_delta_token(c, "microsoft", "users").expect("A's token")
    })
    .delta_link;
    assert_eq!(link, "https://graph.test/a");
}

#[test]
fn directory_contact_in_one_workspace_leaves_the_other_alone() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);
    let user = seeded.a.admin_uuid;

    let contact = |title: &str, phone: &str| DirectoryContact {
        job_title: Some(title.to_string()),
        organization: None,
        department: None,
        office_location: None,
        phones: vec![(phone.to_string(), "work".to_string())],
        address: Some(DirectoryAddress {
            street: None,
            city: Some("Sydney".to_string()),
            region: None,
            postal_code: None,
            country: None,
        }),
    };

    as_sync(&pool, Some(a), |c| {
        user_contact::apply_directory_contact(
            c,
            user,
            "microsoft",
            &contact("Engineer", "+61 2 5550 0001"),
            None,
        )
        .expect("A's contact");
        // A field an admin set by hand in A.
        diesel::update(
            user_profiles::table
                .filter(user_profiles::workspace_id.eq(a))
                .filter(user_profiles::user_uuid.eq(user)),
        )
        .set(user_profiles::custom_fields.eq(json!({ "badge": "A-7" })))
        .execute(c)
        .expect("set A's field");
    });
    as_sync(&pool, Some(b), |c| {
        user_contact::apply_directory_contact(
            c,
            user,
            "microsoft",
            &contact("Manager", "+61 2 5550 0002"),
            None,
        )
        .expect("B's contact");
    });

    let phones = |ws: i32| {
        as_sync(&pool, Some(ws), |c| {
            user_phone_numbers::table
                .filter(user_phone_numbers::workspace_id.eq(ws))
                .filter(user_phone_numbers::user_uuid.eq(user))
                .select(user_phone_numbers::phone)
                .load::<String>(c)
                .expect("phones")
        })
    };
    assert_eq!(phones(a), vec!["+61 2 5550 0001".to_string()]);
    assert_eq!(phones(b), vec!["+61 2 5550 0002".to_string()]);

    let addresses_in_a: i64 = as_sync(&pool, Some(a), |c| {
        user_addresses::table
            .filter(user_addresses::workspace_id.eq(a))
            .filter(user_addresses::user_uuid.eq(user))
            .count()
            .get_result(c)
            .expect("addresses")
    });
    assert_eq!(addresses_in_a, 1);

    let (title, fields) = as_sync(&pool, Some(b), |c| {
        user_profiles::table
            .filter(user_profiles::workspace_id.eq(b))
            .filter(user_profiles::user_uuid.eq(user))
            .select((user_profiles::job_title, user_profiles::custom_fields))
            .first::<(Option<String>, serde_json::Value)>(c)
            .expect("B's profile")
    });
    assert_eq!(title.as_deref(), Some("Manager"));
    assert!(
        fields.get("badge").is_none(),
        "B's profile doesn't start from A's: {fields}"
    );
}
