//! Aggregate registry.
//!
//! Single source of truth for the schema version of each aggregate's
//! event payload. The version stamps every `sync_actions` row so
//! consumers can switch on `schema_version` when the payload shape
//! evolves. Bumping a version is a code change here plus a
//! consumer-side switch arm; no migration required.
//!
//! When the JSON model manifests + `build.rs` codegen land in a later
//! commit this file becomes generated.

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
    }
}
