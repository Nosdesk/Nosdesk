//! Remove imported assets (design r2 §9): delete the assets an import
//! created that nobody has touched since, and the manufacturers, models
//! and groups it created that nothing uses any more.
//!
//! An asset is untouched when every check of [`KeptReason`] passes: its
//! `updated_at` is the one the commit read back, its newest lifecycle
//! event is the one the import wrote, it has no loan, disposal, photo or
//! file, audit, usage entry, ticket link or directory membership, it is in
//! no group the import didn't add it to, no sync source owns it, and no
//! other asset's asset-reference attribute names it.
//!
//! The rule reads the job's committed row results, which the 30-day prune
//! drops. Removal is open for the same 30 days from the commit
//! ([`window_closes_at`]) and refuses a pruned job, so it never runs
//! without them.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Mutex;

use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::result::Error as DieselError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::repository::assets::AssetDeletedObserver;
use crate::repository::{
    asset_groups as group_repo, asset_kinds as kind_repo, asset_models as model_repo,
    assets as asset_repo, import_removal as repo, manufacturers as maker_repo,
};

use super::history;
use super::{CatalogKind, RecordId, RowOutcome, RowResult};

/// When removal closes for a job committed at `completed_at`: the end of
/// the window its details are kept for.
pub fn window_closes_at(completed_at: DateTime<Utc>) -> DateTime<Utc> {
    completed_at + history::details_window()
}

/// Why an asset the import created is kept. A kept asset carries every
/// reason that applies, most telling first (the order of the variants);
/// the first is the one the page shows.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum KeptReason {
    /// `external_sync_source` is set: "Now synced from Intune".
    Synced { source: String },
    /// A disposal record: "Disposed".
    Disposed,
    /// A loan record: "Has a loan record".
    Loan,
    /// A lifecycle event newer than the import's own: "Status changed
    /// since".
    StatusChanged,
    /// A ticket link: "Linked to ticket #412".
    Ticket { ticket_id: i32 },
    /// Photos or files: "Has 2 photos".
    Media { photos: i64, files: i64 },
    /// A stock count: "Audited on 2 Nov".
    Audited { at: DateTime<Utc> },
    /// A usage entry: "Usage logged".
    Usage,
    /// A directory membership: "In a directory group".
    Directory,
    /// Another asset's asset-reference attribute names it: "Referenced by
    /// 'Tripod kit 4'".
    Referenced { asset_id: i32, name: String },
    /// A group assignment the import didn't make: "Added to a group since".
    GroupAdded,
    /// `updated_at` moved: "Edited on 3 Nov", with who from the audit log
    /// when it has them. `by_name` is filled when the result is read.
    Edited {
        at: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by_name: Option<String>,
    },
}

/// An asset the import created that stays, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptAsset {
    pub id: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    pub name: String,
    pub reasons: Vec<KeptReason>,
}

/// How many catalog entries the import created that still exist unchanged
/// and so are removed when nothing uses them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogCounts {
    pub manufacturers: usize,
    pub models: usize,
    pub groups: usize,
}

/// What removal would do now: the dialog's counts and kept list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemovalPlan {
    /// Assets the import created.
    pub created: usize,
    /// Of those, the ones that would be deleted.
    pub deletable: usize,
    /// Of those, the ones already deleted some other way.
    pub gone: usize,
    pub kept: Vec<KeptAsset>,
    pub catalog: CatalogCounts,
    /// The ids `deletable` counts, for the removal to delete.
    #[serde(skip)]
    pub deletable_ids: Vec<i32>,
}

/// What became of the catalog entries of one kind the import created.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryOutcome {
    #[serde(default)]
    pub deleted: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archived: Vec<i32>,
    /// Still used, or changed since the import.
    #[serde(default)]
    pub kept: Vec<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogOutcome {
    pub manufacturers: EntryOutcome,
    pub models: EntryOutcome,
    pub groups: EntryOutcome,
}

/// What removal did, as `import_jobs.remove_result` keeps it. The job's
/// audit entry carries it, kept list included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveResult {
    pub created: usize,
    pub deleted: usize,
    #[serde(default)]
    pub gone: usize,
    pub deleted_ids: Vec<i32>,
    pub kept: Vec<KeptAsset>,
    pub catalog: CatalogOutcome,
}

impl RemoveResult {
    /// The people the kept list names, so a reader can fill in their names.
    pub fn editors(&self) -> Vec<Uuid> {
        let mut out: Vec<Uuid> = self
            .kept
            .iter()
            .flat_map(|k| &k.reasons)
            .filter_map(|r| match r {
                KeptReason::Edited { by, .. } => *by,
                _ => None,
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Put the editors' names on the kept list.
    pub fn name_editors(&mut self, names: &HashMap<Uuid, String>) {
        for kept in &mut self.kept {
            for reason in &mut kept.reasons {
                if let KeptReason::Edited { by, by_name, .. } = reason {
                    *by_name = by.and_then(|u| names.get(&u).cloned());
                }
            }
        }
    }
}

/// One asset the import created, as its row result recorded it.
#[derive(Debug, Clone)]
struct Candidate {
    id: i32,
    observed: Option<NaiveDateTime>,
    transition_id: Option<i32>,
    groups_added: Vec<i32>,
    written_at: Option<DateTime<Utc>>,
}

fn candidates(rows: &[RowResult]) -> Vec<Candidate> {
    let mut seen = HashSet::new();
    rows.iter()
        .filter(|r| r.outcome == RowOutcome::Create)
        .filter_map(|r| match r.record_id {
            Some(RecordId::Int(id)) if seen.insert(id) => Some(Candidate {
                id,
                observed: r.observed_updated_at.as_deref().and_then(parse_observed),
                transition_id: r.transition_id,
                groups_added: r.groups_added.clone(),
                written_at: r.written_at,
            }),
            _ => None,
        })
        .collect()
}

/// The catalog entries the import created, by kind, each with the
/// `updated_at` it read back.
fn catalog_created(rows: &[RowResult]) -> HashMap<CatalogKind, Vec<(i32, Option<NaiveDateTime>)>> {
    let mut out: HashMap<CatalogKind, Vec<(i32, Option<NaiveDateTime>)>> = HashMap::new();
    let mut seen = HashSet::new();
    for entry in rows
        .iter()
        .filter(|r| matches!(r.outcome, RowOutcome::Create | RowOutcome::Update))
        .flat_map(|r| &r.catalog_created)
    {
        if seen.insert((entry.kind, entry.id)) {
            out.entry(entry.kind).or_default().push((
                entry.id,
                entry
                    .observed_updated_at
                    .as_deref()
                    .and_then(parse_observed),
            ));
        }
    }
    out
}

/// A row result's `observed_updated_at` (microseconds, as the commit
/// formats it) as a time to compare with the column.
fn parse_observed(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").ok()
}

/// The asset-reference attribute keys of each kind, from the kinds'
/// schemas: properties with `format: asset-ref`.
fn reference_keys(
    conn: &mut DbConnection,
) -> Result<HashMap<String, HashSet<String>>, DieselError> {
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    for kind in kind_repo::list_kinds(conn)? {
        let Some(props) = kind
            .attribute_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        for (key, prop) in props {
            if prop.get("format").and_then(serde_json::Value::as_str) == Some("asset-ref") {
                out.entry(kind.slug.clone())
                    .or_default()
                    .insert(key.clone());
            }
        }
    }
    Ok(out)
}

/// Check every asset the import created against the untouched rule, and
/// count the catalog entries removal would try. With `lock`, the assets
/// and catalog entries are locked for the rest of the transaction (see
/// [`repo::asset_states`]).
pub fn plan(
    conn: &mut DbConnection,
    rows: &[RowResult],
    lock: bool,
) -> Result<RemovalPlan, DieselError> {
    let candidates = candidates(rows);
    let ids: Vec<i32> = candidates.iter().map(|c| c.id).collect();
    let states: HashMap<i32, repo::AssetState> = repo::asset_states(conn, &ids, lock)?
        .into_iter()
        .map(|s| (s.id, s))
        .collect();
    let live: Vec<i32> = ids
        .iter()
        .copied()
        .filter(|id| states.contains_key(id))
        .collect();

    let events = repo::newest_lifecycle_events(conn, &live)?;
    let loans = repo::loan_counts(conn, &live)?;
    let disposals = repo::disposal_counts(conn, &live)?;
    let media = repo::media_counts(conn, &live)?;
    let audits = repo::latest_audits(conn, &live)?;
    let usage = repo::usage_counts(conn, &live)?;
    let groups = repo::group_assignments(conn, &live)?;
    let tickets = repo::linked_tickets(conn, &live)?;
    let directory = repo::directory_counts(conn, &live)?;
    let references = references_to(conn, &live)?;

    let mut kept = Vec::new();
    let mut deletable_ids = Vec::new();
    let mut edited = Vec::new();
    for c in &candidates {
        let Some(state) = states.get(&c.id) else {
            continue;
        };
        let mut reasons = Vec::new();
        if let Some(source) = state
            .external_sync_source
            .as_deref()
            .filter(|s| !s.is_empty())
        {
            reasons.push(KeptReason::Synced {
                source: source.to_string(),
            });
        }
        if disposals.get(&c.id).copied().unwrap_or(0) > 0 {
            reasons.push(KeptReason::Disposed);
        }
        if loans.get(&c.id).copied().unwrap_or(0) > 0 {
            reasons.push(KeptReason::Loan);
        }
        match (events.get(&c.id), c.transition_id) {
            (None, _) => {}
            (Some(newest), Some(own)) if *newest == own => {}
            _ => reasons.push(KeptReason::StatusChanged),
        }
        if let Some(ticket) = tickets.get(&c.id) {
            reasons.push(KeptReason::Ticket { ticket_id: *ticket });
        }
        if let Some((photos, files)) = media.get(&c.id) {
            reasons.push(KeptReason::Media {
                photos: *photos,
                files: *files,
            });
        }
        if let Some(at) = audits.get(&c.id) {
            reasons.push(KeptReason::Audited { at: *at });
        }
        if usage.get(&c.id).copied().unwrap_or(0) > 0 {
            reasons.push(KeptReason::Usage);
        }
        if directory.get(&c.id).copied().unwrap_or(0) > 0 {
            reasons.push(KeptReason::Directory);
        }
        if let Some((referrer, name)) = references.get(&c.id) {
            reasons.push(KeptReason::Referenced {
                asset_id: *referrer,
                name: name.clone(),
            });
        }
        if groups
            .get(&c.id)
            .is_some_and(|now| now.iter().any(|g| !c.groups_added.contains(g)))
        {
            reasons.push(KeptReason::GroupAdded);
        }
        if c.observed != Some(state.updated_at) {
            edited.push(c.id);
            reasons.push(KeptReason::Edited {
                at: state.updated_at.and_utc(),
                by: None,
                by_name: None,
            });
        }
        if reasons.is_empty() {
            deletable_ids.push(c.id);
        } else {
            reasons.sort();
            kept.push(KeptAsset {
                id: c.id,
                tag: state.asset_tag.clone().filter(|t| !t.is_empty()),
                name: state.name.clone(),
                reasons,
            });
        }
    }

    // Who made the last edit, from the audit log, after the commit.
    if !edited.is_empty() {
        let since = candidates
            .iter()
            .filter_map(|c| c.written_at)
            .min()
            .unwrap_or(DateTime::<Utc>::MIN_UTC);
        let editors = repo::last_editors(conn, &edited, since)?;
        for k in &mut kept {
            for reason in &mut k.reasons {
                if let KeptReason::Edited { by, .. } = reason {
                    *by = editors.get(&k.id).copied();
                }
            }
        }
    }

    let catalog = catalog_counts(conn, rows)?;
    Ok(RemovalPlan {
        created: candidates.len(),
        deletable: deletable_ids.len(),
        gone: candidates.len() - live.len(),
        kept,
        catalog,
        deletable_ids,
    })
}

/// For each of `ids`, one other asset whose asset-reference attribute
/// names it, with its name.
fn references_to(
    conn: &mut DbConnection,
    ids: &[i32],
) -> Result<HashMap<i32, (i32, String)>, DieselError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let by_kind = reference_keys(conn)?;
    let keys: Vec<String> = by_kind
        .values()
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let targets: Vec<String> = ids.iter().map(ToString::to_string).collect();
    let mut out: HashMap<i32, (i32, String)> = HashMap::new();
    let mut refs = repo::attribute_references(conn, &keys, &targets)?;
    refs.sort_by_key(|r| r.referrer_id);
    for r in refs {
        let is_ref = by_kind
            .get(&r.referrer_kind)
            .is_some_and(|keys| keys.contains(&r.key));
        let Ok(target) = r.value.trim().parse::<i32>() else {
            continue;
        };
        if is_ref && target != r.referrer_id {
            out.entry(target)
                .or_insert((r.referrer_id, r.referrer_name));
        }
    }
    Ok(out)
}

/// The catalog entries the import created that still exist as the import
/// left them.
fn catalog_counts(
    conn: &mut DbConnection,
    rows: &[RowResult],
) -> Result<CatalogCounts, DieselError> {
    let created = catalog_created(rows);
    let unchanged = |states: Vec<repo::CatalogState>, kind: CatalogKind| {
        let observed: HashMap<i32, Option<NaiveDateTime>> = created
            .get(&kind)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();
        states
            .iter()
            .filter(|s| !s.archived && observed.get(&s.id).copied().flatten() == Some(s.updated_at))
            .count()
    };
    let ids = |kind: CatalogKind| -> Vec<i32> {
        created
            .get(&kind)
            .map(|v| v.iter().map(|(id, _)| *id).collect())
            .unwrap_or_default()
    };
    Ok(CatalogCounts {
        manufacturers: unchanged(
            repo::manufacturer_states(conn, &ids(CatalogKind::Manufacturer), false)?,
            CatalogKind::Manufacturer,
        ),
        models: unchanged(
            repo::model_states(conn, &ids(CatalogKind::Model), false)?,
            CatalogKind::Model,
        ),
        groups: unchanged(
            repo::group_states(conn, &ids(CatalogKind::Group), false)?,
            CatalogKind::Group,
        ),
    })
}

/// Collects the ids `delete_device` deleted, so the search index hears of
/// them once the transaction commits rather than before.
#[derive(Default)]
pub struct DeletedAssets(Mutex<Vec<i32>>);

impl DeletedAssets {
    pub fn into_ids(self) -> Vec<i32> {
        self.0.into_inner().unwrap_or_else(|e| e.into_inner())
    }
}

impl AssetDeletedObserver for DeletedAssets {
    fn device_deleted(&self, device_id: i32) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(device_id);
    }
}

/// Delete the assets `plan` found untouched, each through `delete_device`
/// so sync and the audit trigger see it and `observer` hears of it, then
/// the catalog entries the import created that nothing uses any more.
/// Run inside the transaction that made `plan` with `lock`.
pub fn remove(
    conn: &mut DbConnection,
    rows: &[RowResult],
    plan: RemovalPlan,
    observer: &dyn AssetDeletedObserver,
) -> Result<RemoveResult, DieselError> {
    let mut deleted_ids = Vec::with_capacity(plan.deletable_ids.len());
    for id in &plan.deletable_ids {
        if asset_repo::delete_device(conn, *id, Some(observer))? > 0 {
            deleted_ids.push(*id);
        }
    }
    let catalog = remove_catalog(conn, rows)?;
    Ok(RemoveResult {
        created: plan.created,
        deleted: deleted_ids.len(),
        gone: plan.gone,
        deleted_ids,
        kept: plan.kept,
        catalog,
    })
}

/// Models first (an asset links to one), then manufacturers (a model
/// belongs to one; that FK refuses the delete), then groups. An entry is
/// removed only when it still has the `updated_at` the import read back
/// and nothing uses it by id; a group something is still assigned to is
/// archived instead.
fn remove_catalog(
    conn: &mut DbConnection,
    rows: &[RowResult],
) -> Result<CatalogOutcome, DieselError> {
    let created = catalog_created(rows);
    let observed = |kind: CatalogKind| -> HashMap<i32, Option<NaiveDateTime>> {
        created
            .get(&kind)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect()
    };
    let mut outcome = CatalogOutcome::default();

    let models = observed(CatalogKind::Model);
    let ids: Vec<i32> = sorted_keys(&models);
    let states = repo::model_states(conn, &ids, true)?;
    let in_use: HashSet<i32> = repo::models_in_use(conn, &ids)?.into_iter().collect();
    for s in states {
        if models.get(&s.id).copied().flatten() == Some(s.updated_at) && !in_use.contains(&s.id) {
            model_repo::delete(conn, s.id)?;
            outcome.models.deleted.push(s.id);
        } else {
            outcome.models.kept.push(s.id);
        }
    }

    let makers = observed(CatalogKind::Manufacturer);
    let ids: Vec<i32> = sorted_keys(&makers);
    let states = repo::manufacturer_states(conn, &ids, true)?;
    let in_use: HashSet<i32> = repo::manufacturers_in_use(conn, &ids)?
        .into_iter()
        .collect();
    for s in states {
        if makers.get(&s.id).copied().flatten() == Some(s.updated_at) && !in_use.contains(&s.id) {
            maker_repo::delete(conn, s.id)?;
            outcome.manufacturers.deleted.push(s.id);
        } else {
            outcome.manufacturers.kept.push(s.id);
        }
    }

    let groups = observed(CatalogKind::Group);
    let ids: Vec<i32> = sorted_keys(&groups);
    let states = repo::group_states(conn, &ids, true)?;
    let in_use: HashSet<i32> = repo::groups_in_use(conn, &ids)?.into_iter().collect();
    for s in states {
        let unchanged = !s.archived && groups.get(&s.id).copied().flatten() == Some(s.updated_at);
        if !unchanged {
            outcome.groups.kept.push(s.id);
        } else if in_use.contains(&s.id) {
            group_repo::archive_group(conn, s.id)?;
            outcome.groups.archived.push(s.id);
        } else {
            group_repo::delete_group(conn, s.id)?;
            outcome.groups.deleted.push(s.id);
        }
    }
    Ok(outcome)
}

fn sorted_keys<V>(map: &HashMap<i32, V>) -> Vec<i32> {
    let mut ids: Vec<i32> = map.keys().copied().collect();
    ids.sort_unstable();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::imports::CatalogRef;

    fn created(id: i32) -> RowResult {
        let mut r =
            RowResult::planned(id as usize, RowOutcome::Create).with_record(RecordId::Int(id));
        r.observed_updated_at = Some("2026-10-15T01:02:03.123456".to_string());
        r
    }

    #[test]
    fn observed_times_read_back_to_the_microsecond() {
        let at = parse_observed("2026-10-15T01:02:03.123456").expect("parses");
        assert_eq!(
            at.format("%Y-%m-%dT%H:%M:%S%.6f").to_string(),
            "2026-10-15T01:02:03.123456"
        );
        assert!(parse_observed("yesterday").is_none());
    }

    #[test]
    fn candidates_are_the_created_rows_once_each() {
        let mut updated = RowResult::planned(9, RowOutcome::Update).with_record(RecordId::Int(9));
        updated.observed_updated_at = Some("2026-10-15T01:02:03.000000".to_string());
        let skipped = RowResult::planned(10, RowOutcome::Skipped);
        let rows = vec![created(1), created(2), created(1), updated, skipped];
        let ids: Vec<i32> = candidates(&rows).iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn catalog_entries_count_once_from_every_written_row() {
        let mut a = created(1);
        a.catalog_created = vec![CatalogRef {
            kind: CatalogKind::Model,
            id: 7,
            observed_updated_at: Some("2026-10-15T01:02:03.000001".to_string()),
        }];
        let mut b = RowResult::planned(2, RowOutcome::Update).with_record(RecordId::Int(2));
        b.catalog_created = vec![
            CatalogRef {
                kind: CatalogKind::Model,
                id: 7,
                observed_updated_at: None,
            },
            CatalogRef {
                kind: CatalogKind::Manufacturer,
                id: 3,
                observed_updated_at: None,
            },
        ];
        let made = catalog_created(&[a, b]);
        assert_eq!(made[&CatalogKind::Model].len(), 1);
        assert!(made[&CatalogKind::Model][0].1.is_some());
        assert_eq!(made[&CatalogKind::Manufacturer], vec![(3, None)]);
    }

    #[test]
    fn the_most_telling_reason_comes_first() {
        let mut reasons = [
            KeptReason::Edited {
                at: Utc::now(),
                by: None,
                by_name: None,
            },
            KeptReason::Media {
                photos: 2,
                files: 0,
            },
            KeptReason::Ticket { ticket_id: 4 },
        ];
        reasons.sort();
        assert!(matches!(reasons[0], KeptReason::Ticket { .. }));
        assert!(matches!(reasons[2], KeptReason::Edited { .. }));
    }

    #[test]
    fn reasons_serialize_with_their_code() {
        let json = serde_json::to_value(KeptReason::Ticket { ticket_id: 412 }).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "code": "ticket", "ticket_id": 412 })
        );
        let json = serde_json::to_value(KeptReason::StatusChanged).unwrap();
        assert_eq!(json, serde_json::json!({ "code": "status_changed" }));
    }
}
