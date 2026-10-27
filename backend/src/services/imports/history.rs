//! What an import keeps afterwards, and for how long.
//!
//! A committed job keeps everything for [`DETAILS_KEPT_DAYS`]: the file and
//! its grid, every row's result (cells and before and after values), the
//! answers and the full summary. Then the prune
//! (`services::scheduled_jobs::prune_import_history`) drops those and keeps
//! the counts, the per-kind counts, the options, the mapping with the
//! file's header names, and the created and updated record ids, for the life
//! of the workspace.
//!
//! A draft (a job that reached Map and was never committed) is resumable for
//! the same window from its last change. After it, it reads as Expired, and
//! the prune drops its file and detail the same way.
//!
//! Expired is derived, never stored: see [`state_of`].

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use super::mapping::StoredSource;
use super::ImportSummary;

/// The one retention window: files, row results, the error report's
/// source, and how long a draft can be resumed.
pub const DETAILS_KEPT_DAYS: i64 = 30;

pub fn details_window() -> Duration {
    Duration::days(DETAILS_KEPT_DAYS)
}

/// The state Recent imports and the import detail show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Reached Map, not committed, resumable.
    Draft,
    /// Committed.
    Imported,
    /// A draft past the window, or one whose file the prune dropped. It
    /// can't be resumed.
    Expired,
}

/// A job's state from its status, its last change and whether the prune
/// has run on it. `None` for a job that never reached Map (an upload that
/// was refused) or whose commit failed: neither is listed.
///
/// A draft is Expired once it was pruned or its last change is older than
/// the window, so it reads Expired before the prune reaches it. A committed
/// job stays Imported; past the window its detail is counts only.
pub fn state_of(
    status: &str,
    updated_at: DateTime<Utc>,
    pruned_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<JobState> {
    match status {
        "done" => Some(JobState::Imported),
        "parsed" | "checking" | "dry_run_done" | "committing" => {
            if pruned_at.is_some() || now - updated_at > details_window() {
                Some(JobState::Expired)
            } else {
                Some(JobState::Draft)
            }
        }
        _ => None,
    }
}

/// The summary as the prune keeps it: the outcome counts, the fields
/// updates changed and the kinds created, each with its count, and how
/// many values each unmapped column held. The errors (with the cells they
/// quote), the problem groups, the left-out values, the catalog names and
/// the notices go. `None` when there was no summary or it doesn't read.
pub fn pruned_summary(summary: Option<serde_json::Value>) -> Option<serde_json::Value> {
    let s: ImportSummary = serde_json::from_value(summary?).ok()?;
    let kept = ImportSummary {
        row_count: s.row_count,
        would_create: s.would_create,
        would_update: s.would_update,
        unchanged: s.unchanged,
        errors_total: s.errors_total,
        rows_with_errors: s.rows_with_errors,
        changes: s.changes,
        kinds: s.kinds,
        unmapped: s.unmapped,
        ..Default::default()
    };
    serde_json::to_value(kept).ok()
}

/// The source as the prune keeps it: the file's shape and header names,
/// which the read-only mapping and the next import's header memory read,
/// without the sample values or the sheet preview. `None` when there was
/// no source or it doesn't read.
pub fn pruned_source(source: Option<serde_json::Value>) -> Option<serde_json::Value> {
    let mut s: StoredSource = serde_json::from_value(source?).ok()?;
    s.source.preview.clear();
    // The prune removes the grid with the job's other files.
    s.grid = None;
    for header in &mut s.source.headers {
        header.samples.clear();
    }
    serde_json::to_value(s).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(days_ago: i64, now: DateTime<Utc>) -> DateTime<Utc> {
        now - Duration::days(days_ago)
    }

    #[test]
    fn committed_jobs_stay_imported() {
        let now = Utc::now();
        assert_eq!(
            state_of("done", at(400, now), Some(now), now),
            Some(JobState::Imported)
        );
        assert_eq!(
            state_of("done", at(1, now), None, now),
            Some(JobState::Imported)
        );
    }

    #[test]
    fn drafts_expire_by_age_or_prune() {
        let now = Utc::now();
        assert_eq!(
            state_of("parsed", at(29, now), None, now),
            Some(JobState::Draft)
        );
        assert_eq!(
            state_of("dry_run_done", at(31, now), None, now),
            Some(JobState::Expired)
        );
        assert_eq!(
            state_of("dry_run_done", at(1, now), Some(now), now),
            Some(JobState::Expired)
        );
    }

    #[test]
    fn failed_jobs_have_no_state() {
        let now = Utc::now();
        assert_eq!(state_of("failed", now, None, now), None);
        assert_eq!(state_of("pending", now, None, now), None);
    }

    #[test]
    fn the_pruned_summary_keeps_counts_only() {
        let summary = serde_json::json!({
            "row_count": 5,
            "would_create": 2,
            "would_update": 1,
            "unchanged": 1,
            "errors": [{ "row": 4, "column": "name", "message": "'x@example.com' is not a member" }],
            "errors_truncated": false,
            "errors_total": 1,
            "rows_with_errors": 1,
            "error_groups": [{ "column": "name", "message": "m", "count": 1, "rows": [4] }],
            "notices": [{ "code": "no_key", "message": "m", "count": 1 }],
            "catalog": { "manufacturers": ["Sony"], "models": [], "groups": [] },
            "changes": [{ "field": "location", "count": 1 }],
            "kinds": [{ "kind": "camera", "count": 2 }],
            "unmapped": [{ "index": 3, "values": 5 }],
        });
        let kept = pruned_summary(Some(summary)).expect("kept");
        assert_eq!(kept["would_create"], 2);
        assert_eq!(kept["would_update"], 1);
        assert_eq!(kept["unchanged"], 1);
        assert_eq!(kept["rows_with_errors"], 1);
        assert_eq!(kept["kinds"][0]["count"], 2);
        assert_eq!(kept["changes"][0]["field"], "location");
        assert_eq!(kept["errors"], serde_json::json!([]));
        assert_eq!(kept["error_groups"], serde_json::json!([]));
        assert_eq!(kept["notices"], serde_json::json!([]));
        assert!(kept.get("catalog").is_none(), "{kept}");
        assert!(!kept.to_string().contains("example.com"));
    }
}
