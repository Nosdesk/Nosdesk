//! Validate that the JSON manifests in `backend/sync-models/` agree
//! with the Rust source of truth (`models::SyncAggregate` and
//! `sync::registry::schema_version_for`).
//!
//! This is the SOT-drift guardrail until the full build.rs / Vite
//! plugin codegen lands. Adding a SyncAggregate enum variant without
//! a matching manifest fails this test; bumping a schema_version in
//! the registry without bumping it in the manifest also fails.

use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use backend::models::SyncAggregate;
use backend::sync::registry::schema_version_for;

#[derive(Debug, Deserialize)]
struct Manifest {
    name: String,
    schema_version: i16,
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
        Webhook,
        Channel,
        KnowledgeGap,
        DocumentationPage,
        DocumentationCollection,
        Data,
        Notification,
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
