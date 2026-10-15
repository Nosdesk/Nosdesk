//! Validate that the JSON manifests in `backend/sync-models/` agree
//! with the Rust source of truth (`models::SyncAggregate`,
//! `sync::registry::schema_version_for` and `sync::audience::audience`).
//!
//! This is the SOT-drift guardrail until the full build.rs / Vite
//! plugin codegen lands. Adding a SyncAggregate enum variant without
//! a matching manifest fails this test; bumping a schema_version in
//! the registry without bumping it in the manifest also fails. So does
//! emitting an event type a manifest doesn't list, listing one nothing
//! emits, or an asset payload whose fields differ from `asset.json`.

use diesel::prelude::*;
use regex::Regex;
use serde::Deserialize;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

use backend::models::{NewAsset, SyncAggregate};
use backend::repository::assets::{asset_sync_payload, create_device};
use backend::schema::sync_actions;
use backend::sync::actor::ActorContext;
use backend::sync::audience::audience;
use backend::sync::registry::schema_version_for;
use backend::sync::session::{run_in_workspace, with_actor_bypass_context};

use crate::common;

#[derive(Debug, Deserialize)]
struct Manifest {
    name: String,
    schema_version: i16,
    audience: String,
    #[serde(default)]
    fields: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    events: Vec<EventEntry>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EventEntry {
    #[serde(rename = "type")]
    type_: String,
    op: String,
}

fn manifests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sync-models")
}

fn load_manifest(name: &str) -> Manifest {
    let path = manifests_dir().join(format!("{name}.json"));
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("manifest {name}.json missing or unreadable: {e}"));
    serde_json::from_str::<Manifest>(&raw)
        .unwrap_or_else(|e| panic!("manifest {name}.json malformed: {e}"))
}

fn all_aggregates() -> Vec<SyncAggregate> {
    use SyncAggregate::*;
    vec![
        Ticket,
        Project,
        ProjectTicket,
        WorkflowState,
        Comment,
        Attachment,
        Assignment,
        GroupMembership,
        Plugin,
        Cycle,
        CycleTicket,
        User,
        Asset,
        AssetMedia,
        AssetLifecycleEvent,
        Webhook,
        Channel,
        KnowledgeGap,
        DocumentationPage,
        DocumentationCollection,
        Data,
        Notification,
        TicketAsset,
        LinkedTicket,
        TicketReference,
        AssetUsage,
        AssetAudit,
        AssetLoan,
    ]
}

#[test]
fn every_sync_aggregate_has_a_manifest_with_matching_name() {
    for agg in all_aggregates() {
        let m = load_manifest(agg.as_str());
        assert_eq!(
            m.name,
            agg.as_str(),
            "manifest name does not match SyncAggregate variant",
        );
    }
}

#[test]
fn manifest_schema_version_matches_registry() {
    for agg in all_aggregates() {
        let m = load_manifest(agg.as_str());
        let registry_version = schema_version_for(agg);
        assert_eq!(
            m.schema_version,
            registry_version,
            "schema_version mismatch for `{}`: manifest={}, registry={}",
            agg.as_str(),
            m.schema_version,
            registry_version
        );
    }
}

/// Who receives each kind of record is stated once, in `sync::audience`; a
/// manifest says the same.
#[test]
fn manifest_audience_matches_the_code() {
    for agg in all_aggregates() {
        let m = load_manifest(agg.as_str());
        assert_eq!(
            m.audience,
            audience(agg).as_str(),
            "audience mismatch for `{}`",
            agg.as_str()
        );
    }
}

#[test]
fn no_orphan_manifests() {
    let known: HashSet<&'static str> = all_aggregates().iter().map(|a| a.as_str()).collect();
    let dir = manifests_dir();
    for entry in fs::read_dir(&dir).expect("read sync-models dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("manifest filename");
        assert!(
            known.contains(stem),
            "manifest {stem}.json has no matching SyncAggregate variant",
        );
    }
}

#[test]
fn every_event_type_uses_a_known_op() {
    let allowed = ["I", "U", "D", "A"];
    for agg in all_aggregates() {
        let m = load_manifest(agg.as_str());
        for ev in &m.events {
            assert!(
                allowed.contains(&ev.op.as_str()),
                "manifest {}.json event {} has invalid op {:?}",
                agg.as_str(),
                ev.type_,
                ev.op
            );
        }
    }
}

#[test]
fn every_event_type_is_namespaced_to_its_aggregate() {
    // Every event_type should start with either the aggregate's name
    // or a sibling aggregate name (group_membership manifest holds
    // both `group.*` and `group_membership.*` events; that's the only
    // exception).
    for agg in all_aggregates() {
        let m = load_manifest(agg.as_str());
        for ev in &m.events {
            let prefix = ev.type_.split('.').next().unwrap_or("");
            let valid = prefix == agg.as_str()
                || (agg == SyncAggregate::GroupMembership && prefix == "group");
            assert!(
                valid,
                "manifest {}.json event {} is not namespaced to its aggregate",
                agg.as_str(),
                ev.type_
            );
        }
    }
}

/// The non-test source under `src/`, by path. Leaves out test modules and
/// `sync/tests.rs`, so a name used only in a test doesn't count as emitted,
/// and the webhook module, whose event names are a separate public contract.
fn emitting_source() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let test_mod = Regex::new(r"(?m)^#\[cfg\(test\)\]\s*\n\s*(pub(\(crate\))?\s+)?mod \w+")
        .expect("test module pattern");
    let mut out = Vec::new();
    for entry in WalkDir::new(&root).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let rel = path
            .strip_prefix(&root)
            .expect("under src")
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with("services/webhooks/") || rel == "sync/tests.rs" {
            continue;
        }
        let mut src = fs::read_to_string(path).expect("read source");
        if let Some(m) = test_mod.find(&src) {
            src.truncate(m.start());
        }
        out.push((rel, src));
    }
    out
}

/// The manifest that owns an event type, by its prefix: `group.*` sits in
/// `group_membership.json`. None for a prefix that names no aggregate.
fn owning_aggregate(event_type: &str) -> Option<SyncAggregate> {
    let prefix = event_type.split('.').next().unwrap_or("");
    let prefix = if prefix == "group" {
        "group_membership"
    } else {
        prefix
    };
    all_aggregates().into_iter().find(|a| a.as_str() == prefix)
}

/// Every event type a manifest lists appears in the code that emits it. A
/// listed type the code no longer writes (as `asset.linked` was once links
/// moved to `ticket_asset`) fails here.
#[test]
fn every_listed_event_type_is_emitted() {
    let literal = Regex::new(r#""([a-z_]+\.[a-z_.]+)""#).expect("literal pattern");
    let mut in_code: HashSet<String> = HashSet::new();
    for (_, src) in emitting_source() {
        in_code.extend(literal.captures_iter(&src).map(|c| c[1].to_string()));
    }
    let mut missing = Vec::new();
    for agg in all_aggregates() {
        for ev in load_manifest(agg.as_str()).events {
            if !in_code.contains(&ev.type_) {
                missing.push(format!("{}.json: {}", agg.as_str(), ev.type_));
            }
        }
    }
    assert!(missing.is_empty(), "listed but never emitted: {missing:#?}");
}

/// Every event type written through `SyncEmit { event_type: "..." }` or an
/// emit helper's `SyncOp::X, "..."` argument is listed in its aggregate's
/// manifest. Types chosen at runtime (an `if` or `match` feeding
/// `event_type`) aren't seen here; the check above still covers them.
#[test]
fn every_emitted_event_type_is_listed() {
    let emits = [
        Regex::new(r#"event_type:\s*"([a-z_]+\.[a-z_.]+)""#).expect("field pattern"),
        Regex::new(r#"SyncOp::\w+\s*,\s*"([a-z_]+\.[a-z_.]+)""#).expect("helper pattern"),
    ];
    let mut unlisted = Vec::new();
    for (rel, src) in emitting_source() {
        for re in &emits {
            for c in re.captures_iter(&src) {
                let event_type = &c[1];
                let Some(agg) = owning_aggregate(event_type) else {
                    continue;
                };
                let listed = load_manifest(agg.as_str())
                    .events
                    .iter()
                    .any(|e| e.type_ == event_type);
                if !listed {
                    unlisted.push(format!("{event_type} ({rel})"));
                }
            }
        }
    }
    assert!(unlisted.is_empty(), "emitted but not listed: {unlisted:#?}");
}

fn asset_manifest_fields() -> BTreeSet<String> {
    load_manifest("asset").fields.keys().cloned().collect()
}

fn keys(value: &serde_json::Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("a JSON object")
        .keys()
        .cloned()
        .collect()
}

/// The asset payload, as built and as it lands in `sync_actions`, has
/// exactly the manifest's fields. Bootstrap sends the same payload.
#[test]
fn asset_payload_fields_match_the_manifest() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let reference = "test:sync_model_registry";

    let asset = run_in_workspace(&pool, reference, seeded.a.workspace_id, |c| {
        create_device(
            c,
            NewAsset {
                name: "Manifest check".to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: None,
                notes: None,
                primary_user_uuid: None,
                purchase_date: None,
                asset_tag: None,
                kind: "device".to_string(),
                attributes: serde_json::json!({}),
                quantity: None,
                unit: None,
                external_sync_source: None,
                low_stock_threshold: None,
                created_by: None,
            },
        )
    })
    .expect("create an asset");

    let expected = asset_manifest_fields();
    assert_eq!(
        keys(&asset_sync_payload(&asset)),
        expected,
        "asset_sync_payload"
    );

    let mut conn = pool.get().expect("conn");
    let emitted: serde_json::Value =
        with_actor_bypass_context(&mut conn, &ActorContext::system(reference), |c| {
            sync_actions::table
                .filter(sync_actions::aggregate_id.eq(asset.id.to_string()))
                .filter(sync_actions::event_type.eq("asset.created"))
                .select(sync_actions::data)
                .first(c)
        })
        .expect("the asset.created row");
    assert_eq!(keys(&emitted), expected, "the asset.created row's data");
}

/// The app's lazy fetch builds pool rows with the manifest's fields, the
/// same as bootstrap and the live stream.
#[test]
fn lazy_asset_rows_match_the_manifest() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../frontend/src/sync/lifecycle.ts");
    let ts = fs::read_to_string(&path).expect("read lifecycle.ts");
    let start = ts
        .find("function toAssetCacheRow(")
        .expect("toAssetCacheRow");
    let body_start = start + ts[start..].find("return {").expect("returned row");
    let body = &ts[body_start..body_start + ts[body_start..].find("\n  }").expect("end of row")];
    let fields: BTreeSet<String> = Regex::new(r"(?m)^\s+(\w+):")
        .expect("key pattern")
        .captures_iter(body)
        .map(|c| c[1].to_string())
        .collect();
    assert_eq!(fields, asset_manifest_fields(), "toAssetCacheRow");
}
