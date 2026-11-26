//! On hosted, a helpdesk import seats no staff (the control plane owns staff
//! seats), and what it writes stays in the workspace it imports into: no
//! account from elsewhere is renamed or given an address, every address it
//! adds is an unverified primary, and the organization and phone it carries
//! land on this workspace's profiles only, with no sign-in rule or sharing
//! setting touched.
//!
//! Part of the hosted binary rather than `tests/it`: the deployment mode is
//! process-wide environment.

use std::collections::{BTreeMap, HashMap};

use chrono::{TimeZone, Utc};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Integer, Text};
use uuid::Uuid;

use crate::common;
use backend::services::imports::helpdesk::bundle::{
    Body, BodyFormat, Bundle, Comment, Manifest, Organization, PeopleSource, Person, PersonRole,
    StatusKind, Ticket, FORMAT_VERSION, PERSON_DETAIL,
};
use backend::services::imports::helpdesk::loader::{self, LoadOptions, NumberPolicy};

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn count(conn: &mut common::TestPooledConn, sql: &str) -> i64 {
    diesel::sql_query(sql)
        .get_result::<Count>(conn)
        .expect("count")
        .n
}

fn person(id: &str, email: &str, name: &str, role: PersonRole) -> Person {
    Person {
        source_id: id.into(),
        email: Some(email.into()),
        name: name.into(),
        role,
        active: true,
        deleted: false,
        limited_staff: false,
        custom_role: false,
        organization: Some("Customer Co".into()),
        organization_id: Some("o1".into()),
        phone: Some("+61299990000".into()),
        shared_phone: false,
        created_at: None,
        updated_at: None,
    }
}

fn ticket(id: &str, requester: &str, assignee: &str) -> Ticket {
    let at = Utc
        .with_ymd_and_hms(2025, 3, 1, 9, 0, 0)
        .single()
        .expect("date");
    Ticket {
        source_id: id.into(),
        number: id.parse().ok(),
        subject: format!("Ticket {id}"),
        status: StatusKind::Open,
        status_label: "open".into(),
        priority: None,
        ticket_type: None,
        tags: vec![],
        requester: Some(requester.into()),
        assignee: Some(assignee.into()),
        watchers: vec![],
        created_at: at,
        updated_at: at,
        solved_at: None,
        organization_id: Some("o1".into()),
        fields: BTreeMap::new(),
        comments: vec![Comment {
            source_id: format!("c{id}"),
            author: Some(requester.into()),
            public: true,
            body: Body {
                format: BodyFormat::Plain,
                value: "Help".into(),
            },
            created_at: at,
            attachments: vec![],
        }],
    }
}

#[actix_web::test]
async fn hosted_import_seats_no_staff_and_writes_nothing_global() {
    crate::hosted_env();
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a, seeded.b);

    // A requester of workspace B whose address a Zendesk end user also has.
    let mut conn = pool.get().expect("conn");
    let theirs = common::insert_plain_user(&mut conn, "Bea Elsewhere");
    diesel::sql_query(
        "INSERT INTO user_emails (user_uuid, email, email_type, is_primary, is_verified) \
         VALUES ($1, 'bea@customer.test', 'personal', true, true)",
    )
    .bind::<diesel::sql_types::Uuid, _>(theirs)
    .execute(&mut conn)
    .expect("address");
    common::seat(&mut conn, b.workspace_id, theirs, "member");
    let emails_before = count(&mut conn, "SELECT count(*) AS n FROM user_emails");

    let cast = vec![
        person("1", "cara@customer.test", "Cara", PersonRole::Requester),
        person(
            "2",
            "alan@zendesk-staff.test",
            "Alan Agent",
            PersonRole::Admin,
        ),
        person(
            "3",
            "bea@customer.test",
            "Bea At Zendesk",
            PersonRole::Requester,
        ),
    ];
    let tickets = vec![ticket("11", "1", "2"), ticket("12", "3", "2")];
    let dir = tempfile::tempdir().expect("dir");
    let bundle = Bundle::create(dir.path()).expect("bundle");
    let mut w = bundle.people_writer(true).expect("people");
    for p in &cast {
        w.write(p).expect("person");
    }
    w.checkpoint().expect("flush");
    let mut w = bundle.tickets_writer(0).expect("tickets");
    for t in &tickets {
        w.write(t).expect("ticket");
    }
    w.checkpoint().expect("flush");
    let mut w = bundle.organizations_writer().expect("orgs");
    w.write(&Organization {
        source_id: "o1".into(),
        name: "Customer Co".into(),
        domain_names: vec!["customer.test".into()],
        shared_tickets: true,
        deleted: false,
    })
    .expect("org");
    w.checkpoint().expect("flush");
    bundle
        .write_manifest(&Manifest {
            format_version: FORMAT_VERSION,
            source: "zendesk".into(),
            source_scope: "acme".into(),
            extracted_at: Utc::now(),
            counts: BTreeMap::new(),
            notes: vec![],
            people_source: Some(PeopleSource::Api),
            person_detail: PERSON_DETAIL,
            inputs: None,
            fill: None,
        })
        .expect("manifest");

    let job = Uuid::now_v7();
    let opts = LoadOptions {
        job_id: job,
        workspace_id: a.workspace_id,
        actor: a.admin_uuid,
        numbers: NumberPolicy::Auto,
        agent_map: HashMap::new(),
        create_agents: true,
        allow_missing_people: false,
        batch_size: 10,
        people_batch_size: 10,
        cancel: Default::default(),
    };
    let storage: std::sync::Arc<dyn backend::utils::storage::Storage> =
        std::sync::Arc::new(backend::utils::storage::LocalStorage::new(
            tempfile::tempdir()
                .expect("storage")
                .keep()
                .to_string_lossy()
                .into_owned(),
            "/files".into(),
        ));
    let report = loader::load(&pool, storage, &bundle, &opts, &|_| {})
        .await
        .expect("load");
    assert_eq!(report.tickets_written, 2);

    // No staff seat: the admin at Zendesk is an account without one.
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) AS n FROM workspace_members m JOIN user_emails e \
                 ON e.user_uuid = m.user_uuid WHERE e.email = 'alan@zendesk-staff.test' \
                 AND m.workspace_id = {}",
                a.workspace_id
            )
        ),
        0
    );
    assert_eq!(
        report.people.get("former staff, no seat"),
        Some(&1),
        "{:?}",
        report.people
    );

    // The account from elsewhere: not renamed, not given an address.
    #[derive(QueryableByName)]
    struct Name {
        #[diesel(sql_type = Text)]
        name: String,
    }
    let name = diesel::sql_query("SELECT name FROM users WHERE uuid = $1")
        .bind::<diesel::sql_types::Uuid, _>(theirs)
        .get_result::<Name>(&mut conn)
        .expect("name")
        .name;
    assert_eq!(name, "Bea Elsewhere");
    assert_eq!(
        count(
            &mut conn,
            &format!("SELECT count(*) AS n FROM user_emails WHERE user_uuid = '{theirs}'")
        ),
        1
    );
    // Every address the import added is an unverified primary: Cara's,
    // Alan's and the placeholder author's undeliverable one, and no
    // secondary one.
    assert_eq!(
        count(&mut conn, "SELECT count(*) AS n FROM user_emails") - emails_before,
        3
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) AS n FROM user_emails WHERE email LIKE '%@import.invalid' \
             AND is_primary AND NOT is_verified"
        ),
        1
    );
    #[derive(QueryableByName)]
    struct Addr {
        #[diesel(sql_type = Bool)]
        is_primary: bool,
        #[diesel(sql_type = Bool)]
        is_verified: bool,
    }
    for email in ["cara@customer.test", "alan@zendesk-staff.test"] {
        let row =
            diesel::sql_query("SELECT is_primary, is_verified FROM user_emails WHERE email = $1")
                .bind::<Text, _>(email)
                .get_result::<Addr>(&mut conn)
                .expect("address");
        assert!(row.is_primary && !row.is_verified, "{email}");
    }
    // Profiles and phones only in workspace A: none in B, where the
    // account from elsewhere has its seat.
    for table in ["user_profiles", "user_phone_numbers"] {
        assert_eq!(
            count(
                &mut conn,
                &format!(
                    "SELECT count(*) AS n FROM {table} WHERE workspace_id <> {}",
                    a.workspace_id
                )
            ),
            0,
            "{table}"
        );
    }
    assert!(report.profile.organizations >= 2, "{:?}", report.profile);
    // No sign-in rule or sharing setting from the organization's domains.
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) AS n FROM workspace_identity_providers \
             WHERE cardinality(allowed_domains) > 0"
        ),
        0
    );
    #[derive(QueryableByName)]
    struct Sharing {
        #[diesel(sql_type = Bool)]
        on: bool,
    }
    let sharing = diesel::sql_query(
        "SELECT bool_or(portal_share_by_domain) AS on FROM site_settings WHERE workspace_id = $1",
    )
    .bind::<Integer, _>(a.workspace_id)
    .get_result::<Sharing>(&mut conn)
    .expect("sharing");
    assert!(!sharing.on);
}
