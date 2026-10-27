use std::collections::HashMap;

use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Bool, Text};
use serde_json::json;

use super::*;
use crate::services::docs_markdown::encode::{decode, encode_with_client_id};
use crate::services::docs_markdown::ir::{Block, Document, Inline};
use crate::services::workspace_archive::policy;

const ANA: &str = "0190a000-0000-7000-8000-00000000000a";
const ANA_HERE: &str = "0190b000-0000-7000-8000-00000000000a";
const GONE: &str = "0190a000-0000-7000-8000-0000000000ff";
const PAGE: &str = "0190a000-0000-7000-8000-0000000000d1";

/// Archive keys to target keys; anything missing isn't on the target.
#[derive(Default)]
struct Keys {
    users: HashMap<String, String>,
    keys: HashMap<(Entity, String), String>,
}

impl Keys {
    fn with(mut self, entity: Entity, old: &str, new: &str) -> Self {
        if entity == Entity::User {
            self.users.insert(old.into(), new.into());
        } else {
            self.keys.insert((entity, old.into()), new.into());
        }
        self
    }
}

impl Lookup for Keys {
    fn user(&self, uuid: &str) -> Option<String> {
        self.users.get(uuid).cloned()
    }
    fn key(&self, entity: Entity, key: &str) -> Option<String> {
        self.keys.get(&(entity, key.to_string())).cloned()
    }
}

fn row(value: Value) -> Row {
    match value {
        Value::Object(map) => map,
        _ => panic!("a row is an object"),
    }
}

fn reports(tally: Tally) -> Vec<(String, String, Effect, usize)> {
    tally
        .into_reports()
        .into_iter()
        .map(|r| (r.table, r.column, r.effect, r.count))
        .collect()
}

fn rule(state: &str) -> Row {
    row(json!({
        "id": 1, "state": state,
        "actions": [
            {"kind": "set_status", "config": {"workflow_state_id": 3}},
            {"kind": "assign", "config": {"method": "direct", "user_uuid": ANA}},
            {"kind": "assign", "config": {"method": "group", "group_id": 4}},
            {"kind": "add_tags", "config": {"tag_ids": [5, 6]}},
            {"kind": "reply", "config": {"visibility": "public",
                "body": format!("<p>Thanks <span data-mention=\"true\" data-uuid=\"{ANA}\" data-name=\"Ana\">@Ana</span></p>")}},
            {"kind": "set_priority", "config": {"priority": "high"}}
        ],
        "conditions": [{"kind": "and", "children": [
            {"kind": "leaf", "field": "ticket.category_id", "op": "eq", "value": 8},
            {"kind": "not", "child": {"kind": "leaf", "field": "ticket.tag_ids", "op": "has", "value": [5]}},
            {"kind": "leaf", "field": "ticket.priority", "op": "eq", "value": "high"}
        ]}]
    }))
}

fn everything() -> Keys {
    Keys::default()
        .with(Entity::User, ANA, ANA_HERE)
        .with(Entity::WorkflowState, "3", "103")
        .with(Entity::Group, "4", "104")
        .with(Entity::Tag, "5", "105")
        .with(Entity::Tag, "6", "106")
        .with(Entity::Category, "8", "108")
}

#[test]
fn a_rule_follows_its_states_people_groups_tags_and_categories() {
    let mut rule = rule("live");
    let mut tally = Tally::default();
    let out = rewrite_row(
        "rules",
        &mut rule,
        &Context::default(),
        &everything(),
        Some(&mut tally),
    );
    assert_eq!(out, None);
    let actions = &rule["actions"];
    assert_eq!(actions[0]["config"]["workflow_state_id"], json!(103));
    assert_eq!(actions[1]["config"]["user_uuid"], json!(ANA_HERE));
    assert_eq!(actions[2]["config"]["group_id"], json!(104));
    assert_eq!(actions[3]["config"]["tag_ids"], json!([105, 106]));
    let body = actions[4]["config"]["body"].as_str().unwrap();
    assert!(
        body.contains(&format!("data-uuid=\"{ANA_HERE}\"")),
        "{body}"
    );
    assert_eq!(
        actions[5],
        json!({"kind": "set_priority", "config": {"priority": "high"}})
    );
    let conditions = &rule["conditions"][0]["children"];
    assert_eq!(conditions[0]["value"], json!(108));
    assert_eq!(conditions[1]["child"]["value"], json!([105]));
    assert_eq!(conditions[2]["value"], json!("high"));
    assert_eq!(rule["state"], "live");
    assert!(reports(tally).is_empty());
}

#[test]
fn a_rule_naming_something_the_import_leaves_out_turns_draft() {
    // Tag 6 and the group aren't in the archive.
    let lookup = Keys::default()
        .with(Entity::User, ANA, ANA_HERE)
        .with(Entity::WorkflowState, "3", "103")
        .with(Entity::Tag, "5", "105")
        .with(Entity::Category, "8", "108");
    let mut live = rule("live");
    let mut tally = Tally::default();
    rewrite_row(
        "rules",
        &mut live,
        &Context::default(),
        &lookup,
        Some(&mut tally),
    );
    assert_eq!(live["state"], "draft");
    assert_eq!(live["actions"][2]["config"]["group_id"], Value::Null);
    assert_eq!(live["actions"][3]["config"]["tag_ids"], json!([105]));
    assert_eq!(
        reports(tally),
        vec![
            ("rules".into(), "actions".into(), Effect::Cleared, 2),
            ("rules".into(), "state".into(), Effect::RuleDrafted, 1),
        ]
    );

    // An archived rule stays archived; history only clears.
    let mut archived = rule("archived");
    rewrite_row("rules", &mut archived, &Context::default(), &lookup, None);
    assert_eq!(archived["state"], "archived");
    let mut version = rule("live");
    let mut tally = Tally::default();
    rewrite_row(
        "rule_versions",
        &mut version,
        &Context::default(),
        &lookup,
        Some(&mut tally),
    );
    assert_eq!(version["state"], "live");
    assert_eq!(
        reports(tally),
        vec![("rule_versions".into(), "actions".into(), Effect::Cleared, 2)]
    );
}

#[test]
fn rule_history_follows_its_comment_state_person_and_tags() {
    let lookup = everything().with(Entity::Comment, "70", "170");
    let mut application = row(json!({"actions_taken": [
        {"index": 0, "kind": "set_status", "workflow_state_id": 3},
        {"index": 1, "kind": "assign", "assigned_to_uuid": GONE},
        {"index": 2, "kind": "add_tags", "tag_ids_added": [5, 9], "tag_ids_removed": [6]},
        {"index": 3, "kind": "reply", "comment_id": 70}
    ]}));
    let mut tally = Tally::default();
    rewrite_row(
        "rule_applications",
        &mut application,
        &Context::default(),
        &lookup,
        Some(&mut tally),
    );
    let taken = &application["actions_taken"];
    assert_eq!(taken[0]["workflow_state_id"], json!(103));
    assert_eq!(taken[1]["assigned_to_uuid"], Value::Null);
    assert_eq!(taken[2]["tag_ids_added"], json!([105]));
    assert_eq!(taken[2]["tag_ids_removed"], json!([106]));
    assert_eq!(taken[3]["comment_id"], json!(170));
    assert_eq!(
        reports(tally),
        vec![(
            "rule_applications".into(),
            "actions_taken".into(),
            Effect::Cleared,
            2
        )]
    );
}

fn view(scope: &str, scope_id: Value) -> Row {
    row(json!({
        "dataset": "tickets", "scope": scope, "scope_id": scope_id,
        "filter": {
            "predicate": {"combinator": "AND", "children": [
                {"field": "assignee_uuid", "op": "eq", "value": ANA},
                {"field": "requester_uuid", "op": "in", "value": [ANA, GONE]},
                {"combinator": "OR", "children": [
                    {"field": "workflow_state.id", "op": "eq", "value": 3},
                    {"field": "workflow_state.category", "op": "eq", "value": "open"}
                ]}
            ]},
            "quick_filters": ["mine"],
            "scope": {"project_ids": [1, 2], "cycle_ids": [9], "group_ids": "all",
                      "include_archived": false}
        },
        "shape": {"group_by": "status", "cycle_id": 9}
    }))
}

#[test]
fn a_view_drops_the_predicates_and_scope_entries_the_import_leaves_out() {
    let lookup = everything().with(Entity::Project, "1", "101");
    let mut v = view("project", json!("1"));
    let mut tally = Tally::default();
    let out = rewrite_row(
        "saved_views",
        &mut v,
        &Context::default(),
        &lookup,
        Some(&mut tally),
    );
    assert_eq!(out, None);
    assert_eq!(v["scope_id"], "101");
    let filter = &v["filter"];
    assert_eq!(
        filter["predicate"]["children"],
        json!([
            {"field": "assignee_uuid", "op": "eq", "value": ANA_HERE},
            {"combinator": "OR", "children": [
                {"field": "workflow_state.id", "op": "eq", "value": 103},
                {"field": "workflow_state.category", "op": "eq", "value": "open"}
            ]}
        ])
    );
    assert_eq!(filter["scope"]["project_ids"], json!([101]));
    assert_eq!(filter["scope"]["cycle_ids"], "all");
    assert_eq!(filter["scope"]["group_ids"], "all");
    assert_eq!(filter["quick_filters"], json!(["mine"]));
    assert_eq!(v["shape"]["cycle_id"], Value::Null);
    assert_eq!(
        reports(tally),
        vec![
            (
                "saved_views".into(),
                "filter".into(),
                Effect::PredicateDropped,
                3
            ),
            ("saved_views".into(), "shape".into(), Effect::Cleared, 1),
        ]
    );

    // A project view whose project didn't come has nowhere to show.
    let mut orphan = view("project", json!("2"));
    let mut tally = Tally::default();
    let out = rewrite_row(
        "saved_views",
        &mut orphan,
        &Context::default(),
        &lookup,
        Some(&mut tally),
    );
    assert_eq!(out, Some("unresolved_reference"));
    assert!(reports(tally).contains(&(
        "saved_views".into(),
        "scope_id".into(),
        Effect::RowDropped,
        1
    )));

    // A private view's scope is its owner.
    let mut private = view("private", json!(ANA));
    rewrite_row(
        "saved_views",
        &mut private,
        &Context::default(),
        &lookup,
        None,
    );
    assert_eq!(private["scope_id"], ANA_HERE);

    // An asset view's groups and import.
    let lookup = Keys::default().with(Entity::AssetGroup, "4", "44");
    let mut assets = row(
        json!({"dataset": "assets", "scope": "private", "scope_id": null,
        "filter": {"groups": "4,5", "import": "0190a000-0000-7000-8000-000000000123",
                   "status": "active"}}),
    );
    rewrite_row(
        "saved_views",
        &mut assets,
        &Context::default(),
        &lookup,
        None,
    );
    assert_eq!(
        assets["filter"],
        json!({"groups": "44", "status": "active"})
    );
}

#[test]
fn editor_html_follows_tickets_people_pages_and_images() {
    let lookup = Keys::default()
        .with(Entity::Ticket, "41", "9041")
        .with(Entity::User, ANA, ANA_HERE)
        .with(Entity::PageUuid, PAGE, PAGE)
        .with(Entity::Asset, "12", "512");
    let html = format!(
        "<p>See <span data-ticket-link=\"true\" data-ticket-id=\"41\" \
         data-href=\"https://old.example/tickets/id/41\"></span> and \
         <span data-ticket-link=\"true\" data-ticket-id=\"99\" data-href=\"/tickets/12\"></span>, \
         ask <span data-mention=\"true\" data-uuid=\"{ANA}\" data-name=\"Ana\" \
         data-avatar-url=\"/uploads/users/avatars/a.png\">@Ana</span> or \
         <span data-mention=\"true\" data-uuid=\"{GONE}\" data-name=\"Gone\">@Gone</span>.</p>\
         <p><img src=\"/uploads/tickets/41/notes/a.png\" alt=\"diagram\">\
         <img src=\"/api/files/tickets/99/notes/b.png\" alt=\"lost\">\
         <img src=\"/api/files/assets/12/media/p.jpg\"></p>\
         <div data-embedded-document=\"true\" data-document-uuid=\"{PAGE}\" \
         data-document-title=\"Runbook\">Runbook</div>\
         <div data-embedded-document=\"true\" \
         data-document-uuid=\"0190a000-0000-7000-8000-0000000000d2\">Old page</div>\
         <p>@[Ana]({ANA}) @[Gone]({GONE})</p>"
    );
    let mut comment = row(json!({"content": html}));
    let mut tally = Tally::default();
    rewrite_row(
        "comments",
        &mut comment,
        &Context::default(),
        &lookup,
        Some(&mut tally),
    );
    let out = comment["content"].as_str().unwrap();
    let want = format!(
        "<p>See <span data-ticket-link=\"true\" data-ticket-id=\"9041\" \
         data-href=\"/tickets/id/9041\"></span> and /tickets/12, \
         ask <span data-mention=\"true\" data-uuid=\"{ANA_HERE}\" data-name=\"Ana\">@Ana</span> or \
         @Gone.</p>\
         <p><img src=\"/uploads/tickets/9041/notes/a.png\" alt=\"diagram\">lost\
         <img src=\"/api/files/assets/512/media/p.jpg\"></p>\
         <div data-embedded-document=\"true\" data-document-uuid=\"{PAGE}\" \
         data-document-title=\"Runbook\">Runbook</div>\
         <p>Old page</p>\
         <p>@[Ana]({ANA_HERE}) @Gone</p>"
    );
    assert_eq!(out, want);
    assert_eq!(
        reports(tally),
        vec![
            (
                "comments".into(),
                "content".into(),
                Effect::MentionAsText,
                2
            ),
            (
                "comments".into(),
                "content".into(),
                Effect::TicketLinkAsText,
                1
            ),
            ("comments".into(), "content".into(), Effect::ImageAsText, 1),
            ("comments".into(), "content".into(), Effect::EmbedAsText, 1),
        ]
    );
    // The mention and ticket parsers read the rewrite as the target's.
    let mentioned = crate::services::notifications::mentions::parse_mentions(out);
    assert_eq!(mentioned, vec![Uuid::parse_str(ANA_HERE).unwrap()]);
    assert_eq!(
        crate::services::notifications::mentions::parse_ticket_references(out),
        vec![9041]
    );
}

#[test]
fn custom_fields_follow_their_schema() {
    let kinds = [row(json!({"slug": "laptop", "attribute_schema": {
        "type": "object",
        "properties": {
            "owner": {"type": "string", "format": "user-uuid"},
            "dock": {"type": "string", "format": "asset-ref"},
            "spares": {"type": "array", "items": {"type": "string", "format": "asset-ref"}},
            "serial": {"type": "string"}
        }
    }}))];
    let fields = [row(json!({"schema": {"type": "object", "properties": {
        "manager_of": {"type": "string", "format": "user-uuid"}
    }}}))];
    let ctx = Context::from_rows(&kinds, &fields);
    let lookup = Keys::default()
        .with(Entity::User, ANA, ANA_HERE)
        .with(Entity::Asset, "7", "707");
    let mut asset = row(json!({"kind": "laptop", "attributes": {
        "owner": ANA, "dock": "7", "spares": ["7", "8"], "serial": "7"
    }}));
    rewrite_row("assets", &mut asset, &ctx, &lookup, None);
    assert_eq!(
        asset["attributes"],
        json!({"owner": ANA_HERE, "dock": "707", "spares": ["707"], "serial": "7"})
    );
    let mut other_kind = row(json!({"kind": "phone", "attributes": {"owner": ANA}}));
    rewrite_row("assets", &mut other_kind, &ctx, &lookup, None);
    assert_eq!(other_kind["attributes"]["owner"], ANA);
    let mut profile = row(json!({"custom_fields": {"manager_of": GONE}}));
    rewrite_row("user_profiles", &mut profile, &ctx, &lookup, None);
    assert_eq!(profile["custom_fields"]["manager_of"], Value::Null);
}

#[test]
fn a_notification_about_something_left_out_stays_out() {
    let lookup = Keys::default().with(Entity::Ticket, "41", "9041");
    let mut kept = row(json!({"entity_type": "ticket", "entity_id": 41,
        "metadata": {"ticket_id": 41, "ticket_number": 12}, "source_sync_id": 88}));
    assert_eq!(
        rewrite_row(
            "notifications",
            &mut kept,
            &Context::default(),
            &lookup,
            None
        ),
        None
    );
    assert_eq!(kept["entity_id"], json!(9041));
    assert_eq!(
        kept["metadata"],
        json!({"ticket_id": 9041, "ticket_number": 12})
    );
    assert_eq!(kept["source_sync_id"], Value::Null);
    let mut gone =
        row(json!({"entity_type": "asset", "entity_id": 3, "metadata": {"asset_id": 3}}));
    assert_eq!(
        rewrite_row(
            "notifications",
            &mut gone,
            &Context::default(),
            &lookup,
            None
        ),
        Some("unresolved_reference")
    );
    // `ticket_id: 0` marks a notification that isn't about a ticket.
    let mut page =
        row(json!({"entity_type": "other", "entity_id": 5, "metadata": {"ticket_id": 0}}));
    rewrite_row(
        "notifications",
        &mut page,
        &Context::default(),
        &lookup,
        None,
    );
    assert_eq!(page["metadata"]["ticket_id"], json!(0));
}

#[test]
fn a_document_column_is_rewritten_with_its_state_vector() {
    let doc = encode_with_client_id(
        &Document {
            blocks: vec![Block::Paragraph(vec![
                Inline::TicketLink {
                    ticket_id: "41".into(),
                    href: "/tickets/id/41".into(),
                },
                Inline::Mention {
                    uuid: GONE.into(),
                    name: "Gone".into(),
                    avatar_url: None,
                },
            ])],
        },
        1,
    );
    let lookup = Keys::default().with(Entity::Ticket, "41", "9041");
    let mut page = row(json!({
        "yjs_document": bytea_value(&doc),
        "yjs_state_vector": "\\x00",
        "yjs_client_id": 1,
    }));
    let mut tally = Tally::default();
    rewrite_row(
        "documentation_pages",
        &mut page,
        &Context::default(),
        &lookup,
        Some(&mut tally),
    );
    let update = bytea(&page["yjs_document"]).expect("hex");
    let json = decode::to_json(&update);
    assert_eq!(
        json["content"][0]["content"][0]["attrs"]["ticketId"],
        "9041"
    );
    assert_eq!(json["content"][0]["content"][1]["text"], "@Gone");
    let vector = bytea(&page["yjs_state_vector"]).expect("hex");
    let loaded = crate::services::yjs::load(&update).expect("loads");
    assert_eq!(vector, crate::services::yjs::encode(&loaded).1);
    assert_eq!(page["yjs_client_id"], Value::Null);
    assert_eq!(
        reports(tally),
        vec![(
            "documentation_pages".into(),
            "yjs_document".into(),
            Effect::MentionAsText,
            1
        )]
    );

    let mut broken = row(json!({"yjs_document": "\\x0102", "yjs_state_vector": "\\x00"}));
    let mut tally = Tally::default();
    rewrite_row(
        "article_contents",
        &mut broken,
        &Context::default(),
        &lookup,
        Some(&mut tally),
    );
    assert_eq!(broken["yjs_document"], "\\x0102");
    assert_eq!(
        reports(tally),
        vec![(
            "article_contents".into(),
            "yjs_document".into(),
            Effect::NotADocument,
            1
        )]
    );
}

#[test]
fn canonical_keys() {
    assert_eq!(canonical(Entity::Tag, &json!(5)), Some("5".into()));
    assert_eq!(canonical(Entity::Tag, &json!("5")), Some("5".into()));
    assert_eq!(canonical(Entity::Tag, &json!(0)), None);
    assert_eq!(canonical(Entity::Project, &json!("all")), None);
    assert_eq!(
        canonical(Entity::User, &json!(ANA.to_uppercase())),
        Some(ANA.into())
    );
    assert_eq!(canonical(Entity::User, &json!("someone")), None);
}

/// Every JSONB column, Yjs document and id column without a foreign key on
/// a carried workspace table is in the registry or declared free of ids;
/// every entry names a column the schema has; every entity's key exists.
#[test]
fn every_jsonb_document_and_loose_id_column_is_classified() {
    #[derive(QueryableByName)]
    struct Column {
        #[diesel(sql_type = Text)]
        table_name: String,
        #[diesel(sql_type = Text)]
        column_name: String,
        #[diesel(sql_type = Text)]
        kind: String,
    }
    #[derive(QueryableByName)]
    struct Exists {
        #[diesel(sql_type = Bool)]
        exists: bool,
    }
    let pool = crate::test_helpers::setup_test_pool();
    let mut conn = pool.get().expect("conn");
    let columns: Vec<Column> = sql_query(
        "SELECT c.relname::text AS table_name, a.attname::text AS column_name, \
                CASE WHEN format_type(a.atttypid, a.atttypmod) IN ('jsonb', 'json', 'jsonb[]') \
                     THEN 'jsonb' \
                     WHEN format_type(a.atttypid, a.atttypmod) = 'bytea' THEN 'bytea' \
                     ELSE 'loose id' END AS kind \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public' \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
         WHERE c.relkind IN ('r', 'p') AND NOT c.relispartition \
           AND EXISTS (SELECT 1 FROM pg_attribute w WHERE w.attrelid = c.oid \
                       AND w.attname = 'workspace_id' AND NOT w.attisdropped) \
           AND (format_type(a.atttypid, a.atttypmod) IN ('jsonb', 'json', 'jsonb[]', 'bytea') \
                OR (a.attname ~ '(_id|_ids|_ref)$' \
                    AND a.attname <> 'workspace_id' \
                    AND format_type(a.atttypid, a.atttypmod) NOT IN ('uuid', 'uuid[]') \
                    AND NOT EXISTS (SELECT 1 FROM pg_constraint k WHERE k.contype = 'f' \
                                    AND k.conrelid = c.oid AND a.attnum = ANY (k.conkey)))) \
         ORDER BY 1, 2",
    )
    .load(&mut conn)
    .expect("columns");

    let classified = classified();
    let mut unclassified = Vec::new();
    let mut counts: BTreeMap<(String, &str), usize> = BTreeMap::new();
    for col in &columns {
        let key = (col.table_name.as_str(), col.column_name.as_str());
        let class = if !policy::carried(&col.table_name) {
            "not carried"
        } else if REFERENCES.iter().any(|r| (r.table, r.column) == key) {
            "references"
        } else if classified.contains(&key) {
            "no ids"
        } else if col.column_name.starts_with("encrypted_") || col.column_name.ends_with("kek_id") {
            // Sealed under the source's keyring, or naming its key; the plan
            // drops them.
            "sealed"
        } else {
            unclassified.push(format!(
                "{}.{} ({})",
                col.table_name, col.column_name, col.kind
            ));
            "unclassified"
        };
        *counts.entry((col.kind.clone(), class)).or_default() += 1;
    }
    println!("classification: {counts:?}");
    assert!(
        unclassified.is_empty(),
        "columns on carried tables that may hold ids no foreign key carries: {unclassified:?}. \
         Add each to refs::REFERENCES, or to refs::NO_IDS with the reason it holds none."
    );

    let exists = |conn: &mut crate::db::DbConnection, table: &str, column: &str| {
        sql_query(
            "SELECT EXISTS (SELECT 1 FROM pg_attribute \
             WHERE attrelid = to_regclass('public.' || $1) AND attname = $2 \
               AND NOT attisdropped) AS exists",
        )
        .bind::<Text, _>(table)
        .bind::<Text, _>(column)
        .get_result::<Exists>(conn)
        .expect("exists")
        .exists
    };
    for (table, column) in &classified {
        assert!(
            exists(&mut conn, table, column),
            "{table}.{column} isn't in the schema"
        );
    }
    for reference in REFERENCES {
        assert!(
            policy::carried(reference.table),
            "{} isn't carried",
            reference.table
        );
    }
    for entity in Entity::ALL {
        if let Some((table, column)) = entity.key() {
            assert!(
                exists(&mut conn, table, column),
                "{entity:?}: {table}.{column} isn't in the schema"
            );
        }
    }
}

/// Snapshots of collaborative documents never move: the import loads each
/// document's full state instead.
#[test]
fn yjs_snapshots_are_never_carried() {
    assert!(policy::table("yjs_snapshots").is_some_and(|t| !t.class.carried()));
}
