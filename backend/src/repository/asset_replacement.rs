//! Carry-overs from a replaced asset to the one that replaced it.
//!
//! A record whose outcome is in the `replaced` category names the asset
//! that took over. Once it is saved, staff choose which of three things
//! follow the equipment, each through the write path it always uses:
//!
//! - the old asset's open tickets are linked to the new one as well
//!   ([`tickets::add_device_to_ticket`]); the old links stay;
//! - its holder, managed by, location and groups are copied to the new one
//!   ([`assets::update_device`] and [`asset_groups::add_groups_to_asset`]);
//! - the old asset moves to retired, in stock or disposed
//!   ([`asset_lifecycle::transition`]), completing the one open record of
//!   the stay it leaves.
//!
//! Nothing runs unless asked. Everything asked runs in one transaction, so
//! a refusal by any part leaves every part undone. A retry adds no second
//! link, copies nothing already there and moves nothing already moved.

use std::collections::{BTreeSet, HashSet};

use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error};
use serde::Serialize;

use crate::db::DbConnection;
use crate::models::{
    Asset, AssetLifecycleEvent, AssetRecord, AssetStatus, AssetUpdate, OutcomeCategory,
    WorkflowStateCategory,
};
use crate::repository::asset_lifecycle::{
    self, CompleteRecordInput, DisposalInput, TransitionError, TransitionInput,
};
use crate::repository::ticket_visibility::{self, VisibilityContext};
use crate::repository::{
    asset_groups, asset_record_types as types_repo, asset_records as records_repo, assets, tickets,
};
use crate::schema::{asset_records, ticket_assets, tickets as tickets_table, workflow_states};

/// The statuses the old asset may move to.
pub const OLD_ASSET_STATUSES: [AssetStatus; 3] = [
    AssetStatus::Retired,
    AssetStatus::InStock,
    AssetStatus::Disposed,
];

/// The old asset's move: its status, and the disposal record a move to
/// `disposed` carries.
pub struct MoveOld {
    pub status: AssetStatus,
    pub disposal: Option<DisposalInput>,
}

/// Which carry-overs to run. Each is off unless set.
#[derive(Default)]
pub struct CarryOver {
    pub link_open_tickets: bool,
    pub copy_assignment: bool,
    pub move_old_to: Option<MoveOld>,
}

/// The open tickets now linked to the new asset.
#[derive(Debug, Serialize)]
pub struct TicketsCarried {
    /// Linked by this call.
    pub linked: Vec<i32>,
    /// Open and already linked to the new asset.
    pub already_linked: Vec<i32>,
}

/// What the new asset took from the old one.
#[derive(Debug, Serialize)]
pub struct AssignmentCopied {
    /// The fields written: `holder`, `managed_by`, `location`.
    pub copied: Vec<&'static str>,
    /// Fields the old asset has that a synced new asset can't take here:
    /// its holder and location belong to the sync.
    pub skipped: Vec<&'static str>,
    /// Groups the new asset joined.
    pub groups_added: Vec<i32>,
}

/// The old asset's move.
#[derive(Debug, Serialize)]
pub struct OldAssetMoved {
    pub status: String,
    /// The lifecycle event the move wrote; `None` when the asset was
    /// already in the status.
    pub event_id: Option<i32>,
    /// The open record of the stay the move ended, completed with it.
    pub completed_record_id: Option<i32>,
}

/// What a carry-over did, a part for each one asked for.
#[derive(Debug, Serialize)]
pub struct CarryOverResult {
    pub record_id: i32,
    pub asset_id: i32,
    pub replacement_asset_id: i32,
    pub tickets: Option<TicketsCarried>,
    pub assignment: Option<AssignmentCopied>,
    pub old_asset: Option<OldAssetMoved>,
}

#[derive(Debug)]
pub enum ReplacementError {
    /// No such record on this asset.
    RecordNotFound,
    /// The record's outcome isn't in the `replaced` category.
    NotReplaced,
    /// The record names no replacement asset (or it has gone).
    NoReplacementAsset,
    /// The old asset's move was refused.
    Transition(TransitionError),
    Db(Error),
}

impl From<Error> for ReplacementError {
    fn from(e: Error) -> Self {
        ReplacementError::Db(e)
    }
}

impl From<TransitionError> for ReplacementError {
    fn from(e: TransitionError) -> Self {
        ReplacementError::Transition(e)
    }
}

/// Run the carry-overs `input` asks for, from record `record_id` of asset
/// `asset_id` to the asset it names as the replacement. `viewer` is whoever
/// asks: only tickets they can see are linked.
pub fn carry_over(
    conn: &mut DbConnection,
    asset_id: i32,
    record_id: i32,
    input: CarryOver,
    viewer: &VisibilityContext,
    actor: Option<uuid::Uuid>,
) -> Result<CarryOverResult, ReplacementError> {
    conn.transaction::<CarryOverResult, ReplacementError, _>(|conn| {
        // Lock the record so two carry-overs from it run one after the
        // other: the second sees the first one's links.
        let record: AssetRecord = asset_records::table
            .find(record_id)
            .for_no_key_update()
            .first(conn)
            .optional()?
            .filter(|r: &AssetRecord| r.asset_id == asset_id)
            .ok_or(ReplacementError::RecordNotFound)?;
        let replaced = match (
            &record.outcome,
            types_repo::get(conn, record.record_type_id)?,
        ) {
            (Some(key), Some(rtype)) => rtype
                .outcome(key)
                .is_some_and(|o| o.category == OutcomeCategory::Replaced),
            _ => false,
        };
        if !replaced {
            return Err(ReplacementError::NotReplaced);
        }
        let new_id = record
            .replacement_asset_id
            .ok_or(ReplacementError::NoReplacementAsset)?;
        let old = assets::get_device_by_id(conn, asset_id)?;
        let new = match assets::get_device_by_id(conn, new_id) {
            Ok(a) => a,
            Err(Error::NotFound) => return Err(ReplacementError::NoReplacementAsset),
            Err(e) => return Err(e.into()),
        };

        let tickets = if input.link_open_tickets {
            Some(link_open_tickets(conn, old.id, new.id, viewer)?)
        } else {
            None
        };
        let assignment = if input.copy_assignment {
            Some(copy_assignment(conn, &old, &new, actor)?)
        } else {
            None
        };
        let old_asset = match input.move_old_to {
            Some(m) => Some(move_old(conn, &old, record.id, m, actor)?),
            None => None,
        };

        Ok(CarryOverResult {
            record_id: record.id,
            asset_id: old.id,
            replacement_asset_id: new.id,
            tickets,
            assignment,
            old_asset,
        })
    })
}

/// The open tickets linked to `asset_id` that `viewer` can see, by id.
pub fn open_linked_tickets(
    conn: &mut DbConnection,
    asset_id: i32,
    viewer: &VisibilityContext,
) -> QueryResult<Vec<i32>> {
    let linked: Vec<(i32, WorkflowStateCategory)> = ticket_assets::table
        .inner_join(tickets_table::table.on(tickets_table::id.eq(ticket_assets::ticket_id)))
        .inner_join(
            workflow_states::table.on(workflow_states::id.eq(tickets_table::workflow_state_id)),
        )
        .filter(ticket_assets::asset_id.eq(asset_id))
        .select((tickets_table::id, workflow_states::category))
        .load(conn)?;
    let open: Vec<i32> = linked
        .into_iter()
        .filter(|(_, category)| !category.is_terminal())
        .map(|(id, _)| id)
        .collect();
    let visible = ticket_visibility::visible_ticket_ids(conn, viewer, &open)?;
    let mut out: Vec<i32> = open.into_iter().filter(|id| visible.contains(id)).collect();
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn link_open_tickets(
    conn: &mut DbConnection,
    old_id: i32,
    new_id: i32,
    viewer: &VisibilityContext,
) -> QueryResult<TicketsCarried> {
    let open = open_linked_tickets(conn, old_id, viewer)?;
    let on_new: HashSet<i32> = ticket_assets::table
        .filter(ticket_assets::asset_id.eq(new_id))
        .filter(ticket_assets::ticket_id.eq_any(&open))
        .select(ticket_assets::ticket_id)
        .load::<i32>(conn)?
        .into_iter()
        .collect();
    let mut carried = TicketsCarried {
        linked: Vec::new(),
        already_linked: Vec::new(),
    };
    for ticket_id in open {
        if on_new.contains(&ticket_id) {
            carried.already_linked.push(ticket_id);
            continue;
        }
        // Linked from elsewhere since the read: the link's own savepoint
        // rolls back, and the link is there all the same.
        match tickets::add_device_to_ticket(conn, ticket_id, new_id) {
            Ok(_) => carried.linked.push(ticket_id),
            Err(Error::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                carried.already_linked.push(ticket_id)
            }
            Err(e) => return Err(e),
        }
    }
    Ok(carried)
}

/// Copy what the old asset has: a field it leaves empty clears nothing on
/// the new one, and a group the new one is in stays.
fn copy_assignment(
    conn: &mut DbConnection,
    old: &Asset,
    new: &Asset,
    actor: Option<uuid::Uuid>,
) -> QueryResult<AssignmentCopied> {
    // A synced asset's holder and location are the sync's, as an edit of
    // the asset itself finds them; managed by is Nosdesk's own.
    let synced = new.external_sync_source.is_some();
    let mut update = AssetUpdate::default();
    let mut copied = Vec::new();
    let mut skipped = Vec::new();

    if let Some(holder) = old.primary_user_uuid {
        if synced {
            skipped.push("holder");
        } else if new.primary_user_uuid != Some(holder) {
            update.primary_user_uuid = Some(Some(holder));
            copied.push("holder");
        }
    }
    if let Some(manager) = old.managed_by_user_uuid {
        if new.managed_by_user_uuid != Some(manager) {
            update.managed_by_user_uuid = Some(Some(manager));
            copied.push("managed_by");
        }
    }
    if let Some(location) = old.location.as_deref().filter(|l| !l.trim().is_empty()) {
        if synced {
            skipped.push("location");
        } else if new.location.as_deref() != Some(location) {
            update.location = Some(Some(location.to_string()));
            copied.push("location");
        }
    }
    if !copied.is_empty() {
        assets::update_device(conn, new.id, update)?;
    }

    let old_groups: BTreeSet<i32> = asset_groups::groups_for_asset(conn, old.id)?
        .into_iter()
        .map(|g| g.id)
        .collect();
    let new_groups: BTreeSet<i32> = asset_groups::groups_for_asset(conn, new.id)?
        .into_iter()
        .map(|g| g.id)
        .collect();
    // Added only: the new asset keeps every membership it has, archived
    // groups' included.
    let missing: Vec<i32> = old_groups.difference(&new_groups).copied().collect();
    let groups_added = if missing.is_empty() {
        missing
    } else {
        asset_groups::add_groups_to_asset(conn, new.id, &missing, actor)?
    };

    Ok(AssignmentCopied {
        copied,
        skipped,
        groups_added,
    })
}

/// Move the old asset through the normal transition. One already in the
/// status stays there. Leaving a status whose stay has one open record
/// (other than the replacing one) completes it, as the status change
/// offers by default; with several open, which one ended is a person's
/// call, so all stay open.
fn move_old(
    conn: &mut DbConnection,
    old: &Asset,
    replacing_record: i32,
    to: MoveOld,
    actor: Option<uuid::Uuid>,
) -> Result<OldAssetMoved, ReplacementError> {
    let status = to.status.as_str().to_string();
    if old.status == status {
        return Ok(OldAssetMoved {
            status,
            event_id: None,
            completed_record_id: None,
        });
    }
    let open: Vec<i32> = records_repo::open_for_leaving(conn, old.id, &old.status)?
        .into_iter()
        .map(|r| r.id)
        .filter(|id| *id != replacing_record)
        .collect();
    let completing = match open.as_slice() {
        [only] => Some(*only),
        _ => None,
    };
    let (_, event): (Asset, AssetLifecycleEvent) = asset_lifecycle::transition(
        conn,
        TransitionInput {
            asset_id: old.id,
            to_status: status.clone(),
            reason: None,
            ticket_id: None,
            metadata: serde_json::json!({}),
            actor_uuid: actor,
            disposal: to.disposal,
            record: None,
            complete_record: completing.map(|id| CompleteRecordInput {
                record_id: Some(id),
                ..Default::default()
            }),
        },
    )?;
    Ok(OldAssetMoved {
        status,
        event_id: Some(event.id),
        completed_record_id: completing,
    })
}
