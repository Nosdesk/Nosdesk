//! Restore previous values (design §1): a new asset draft that puts back
//! the values a committed import's updates replaced.
//!
//! The draft's file is the previous values file (`previous`) with one
//! filter: a value goes in only where the asset still holds what the import
//! wrote, so a field someone changed since is left as it is and counted.
//! The draft gets the file as its own upload, is mapped column for column
//! (every header is a field id), matches on the source's key, only updates,
//! and keeps a manufacturer or model the catalog doesn't hold as text, as
//! the asset had it. The handler checks it at once, so it opens on Review.
//!
//! `import_jobs.restore_of` names the source, so a second press while the
//! draft is open finds it, and each detail page links to the other.

use std::collections::HashSet;

use uuid::Uuid;

use super::history::{state_of, JobState};
use super::mapping::ColumnMapping;
use super::types::{
    FieldSpec, ImportMode, ImportOptions, MatchBy, Notice, ValueAnswer, ValueAnswers,
};
use super::values::{Layer, LookupKind, Resolution};
use crate::models::JobLink;
use crate::repository::imports::RestoreJobRow;

/// The notice code Review words as "N values changed since this import".
pub const CHANGED_SINCE: &str = "restore_changed_since";

/// A draft restoring the source that can still go on.
pub fn open_draft(restores: &[RestoreJobRow]) -> Option<Uuid> {
    let now = chrono::Utc::now();
    restores
        .iter()
        .filter(|j| state_of(&j.status, j.updated_at, j.pruned_at, now) == Some(JobState::Draft))
        .map(|j| j.id)
        .next_back()
}

/// The newest committed restore of the source, for its detail page.
pub fn restored_by(restores: &[RestoreJobRow]) -> Option<JobLink> {
    restores
        .iter()
        .filter(|j| j.status == "done")
        .filter_map(|j| j.completed_at.map(|at| JobLink { id: j.id, at }))
        .max_by_key(|l| l.at)
}

/// Each header of the file mapped to the field it names. A header that
/// names no field is left out (the file only holds fields).
pub fn mapping(headers: &[String], fields: &[FieldSpec]) -> ColumnMapping {
    let known: HashSet<&str> = fields.iter().map(|f| f.id.as_str()).collect();
    headers
        .iter()
        .enumerate()
        .filter(|(_, h)| known.contains(h.as_str()))
        .map(|(i, h)| (h.clone(), i))
        .collect()
}

/// Update the assets the source matched, on its key, creating nothing.
pub fn options(key: MatchBy) -> ImportOptions {
    ImportOptions {
        mode: ImportMode::UpdateOnly,
        match_by: key,
        ..Default::default()
    }
}

/// Answer Keep as text for every manufacturer and model value the catalog
/// doesn't hold as written: it was the asset's own text, and a guess or a
/// saved match would link it to something else. Values already answered
/// keep their answer. Whether any answer was added: a manufacturer answer
/// changes the models' contexts, so the caller resolves again until none
/// is.
pub fn keep_text(resolution: &Resolution, answers: &mut ValueAnswers) -> bool {
    let mut added = false;
    for column in &resolution.columns {
        if !matches!(
            column.matcher.lookup(),
            Some(LookupKind::Manufacturer | LookupKind::Model)
        ) {
            continue;
        }
        for value in &column.values {
            if value.key.is_empty() || value.layer == Layer::Exact {
                continue;
            }
            let slot = answers
                .entry(column.field.clone())
                .or_default()
                .entry(column.context.clone())
                .or_default();
            if !slot.contains_key(&value.key) {
                slot.insert(value.key.clone(), ValueAnswer::KeepText);
                added = true;
            }
        }
    }
    added
}

/// Review's line for the values left as they are.
pub fn changed_since_notice(count: usize) -> Notice {
    let message = if count == 1 {
        "1 value changed since this import. It's left as it is.".to_string()
    } else {
        format!("{count} values changed since this import. They're left as they are.")
    };
    Notice {
        code: CHANGED_SINCE.to_string(),
        count,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::imports::types::{FieldGroup, ImportType, ValueType};

    fn job(status: &str, completed: Option<i64>) -> RestoreJobRow {
        let now = chrono::Utc::now();
        RestoreJobRow {
            id: Uuid::now_v7(),
            status: status.to_string(),
            updated_at: now,
            completed_at: completed.map(|days| now - chrono::Duration::days(days)),
            pruned_at: None,
        }
    }

    #[test]
    fn an_open_draft_is_found_and_the_newest_commit_links() {
        let older = job("done", Some(3));
        let newer = job("done", Some(1));
        let draft = job("dry_run_done", None);
        let restores = [older, newer.clone(), draft.clone()];
        assert_eq!(open_draft(&restores), Some(draft.id));
        assert_eq!(restored_by(&restores).map(|l| l.id), Some(newer.id));

        let mut stale = job("dry_run_done", None);
        stale.updated_at = chrono::Utc::now() - chrono::Duration::days(31);
        assert_eq!(open_draft(&[stale]), None);
        assert_eq!(restored_by(&[job("parsed", None)]), None);
    }

    #[test]
    fn every_header_maps_to_the_field_it_names() {
        let field = |id: &str| {
            FieldSpec::fixed(
                ImportType::Assets,
                id,
                id,
                FieldGroup::Core,
                ValueType::Text,
                false,
            )
        };
        let fields = [field("asset_tag"), field("location"), field("notes")];
        let headers = ["asset_tag", "attr:gone", "location"].map(str::to_string);
        let mapped = mapping(&headers, &fields);
        assert_eq!(mapped.get("asset_tag"), Some(&0));
        assert_eq!(mapped.get("location"), Some(&2));
        assert_eq!(mapped.len(), 2);
    }

    #[test]
    fn the_options_only_update_on_the_source_key() {
        let o = options(MatchBy::SerialNumber);
        assert_eq!(o.mode, ImportMode::UpdateOnly);
        assert_eq!(o.match_by, MatchBy::SerialNumber);
        assert!(o.constants.is_empty());
    }
}
