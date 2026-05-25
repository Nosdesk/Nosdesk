//! Aggregate registry.
//!
//! Single source of truth for the schema version of each aggregate's
//! event payload. The version stamps every `sync_actions` row so
//! consumers can switch on `schema_version` when the payload shape
//! evolves. Bumping a version is a code change here plus a
//! consumer-side switch arm; no migration required.
//!
//! Stays in sync with `backend/sync-models/<name>.json` via
//! `tests/sync_model_registry.rs`, which fails the build if a
//! manifest's `schema_version` drifts from the registry. A future
//! `build.rs` pass will generate this file from the manifests
//! directly; until then the registry test is the SOT-drift guard.

use crate::models::SyncAggregate;

pub fn schema_version_for(aggregate: SyncAggregate) -> i16 {
    match aggregate {
        SyncAggregate::Ticket => 1,
        SyncAggregate::Project => 1,
        SyncAggregate::ProjectTicket => 1,
        SyncAggregate::WorkflowState => 1,
        SyncAggregate::Comment => 1,
        SyncAggregate::Attachment => 1,
        SyncAggregate::Assignment => 1,
        SyncAggregate::GroupMembership => 1,
        SyncAggregate::Plugin => 1,
        SyncAggregate::Cycle => 1,
        SyncAggregate::CycleTicket => 1,
        SyncAggregate::User => 1,
        SyncAggregate::Asset => 1,
        SyncAggregate::Webhook => 1,
        SyncAggregate::Channel => 1,
        SyncAggregate::KnowledgeGap => 1,
    }
}
