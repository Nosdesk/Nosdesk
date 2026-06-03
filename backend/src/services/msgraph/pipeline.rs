//! Sync pipeline glue: the typed-error / typed-outcome layer that
//! wraps the existing `crate::handlers::msgraph_integration` sync
//! functions.
//!
//! The pre-pipeline error-collection pattern was:
//! ```ignore
//! let error_msg = format!("Failed to ... user {}: {}", user.name, e);
//! warn!("{}", error_msg);          // leaks user.name into the message body
//! stats.errors.push(error_msg);    // string-typed, classifier-free
//! ```
//! ten times across users / devices / groups. That conflated four
//! responsibilities — emit a log line, classify the error, identify
//! the failing item, persist the failure for the admin UI — into
//! one ad-hoc snippet, and leaked user-typed content (`user.name`,
//! `device_name`, `group_name`) into both the log body and the
//! Vec<String> the admin UI eventually displays.
//!
//! [`record_failure`] centralises the same job in one well-typed
//! call. The caller hands it the failing entity, the upstream
//! stable identifier, and the typed `MsGraphSyncError`; the helper
//! emits a structured-fields-only `tracing::warn` event (no user-
//! typed content in the body) and returns an [`ItemFailure`] for
//! the caller to push into its outcome list.
//!
//! Call sites end up looking like:
//! ```ignore
//! match upsert_user(conn, &ms_user).await {
//!     Ok(()) => stats.existing_users_updated += 1,
//!     Err(e) => stats
//!         .failures
//!         .push(pipeline::record_failure(EntityKind::Users, &ms_user.id, e, 1)),
//! }
//! ```

use tracing::warn;

use super::error::MsGraphSyncError;
use super::outcome::{EntityKind, ItemFailure};

/// Emit a structured warn for a per-item failure and produce the
/// matching `ItemFailure`. One call, four responsibilities done
/// uniformly:
///
///   1. Bounded-fields tracing event with `entity`, `external_id`,
///      `error_kind`, `classification`, `attempt`. No user-typed
///      content in the message body — Display of the error is
///      classifier-only by design.
///   2. The returned `ItemFailure` carries the same context for
///      the admin-UI / outcome aggregate.
///   3. The `error_kind` + `classification` tags route to the right
///      operator playbook (transient -> wait for next tick;
///      permanent -> investigate upstream; conflict -> wait for
///      racing state; auth -> page operator).
///   4. The pipeline reads `attempt` to distinguish a one-shot
///      failure from one that survived the retry executor.
///
/// `external_id` is borrowed because it's already in a struct on
/// the caller's stack; the helper clones it onto the failure
/// record so the caller can drop the source struct without a
/// lifetime fight.
pub fn record_failure(
    entity: EntityKind,
    external_id: &str,
    error: MsGraphSyncError,
    attempt: u32,
) -> ItemFailure {
    let cls = error.classify();
    warn!(
        entity = entity.as_str(),
        external_id,
        attempt,
        error_kind = error.kind_str(),
        classification = cls.as_str(),
        "msgraph sync item failed"
    );
    ItemFailure {
        entity,
        external_id: external_id.to_string(),
        error,
        attempt,
    }
}

/// Aggregate a slice of `ItemFailure`s into structured tracing
/// summaries by classification. Useful at the end-of-run boundary
/// when the executor has the whole outcome in hand: one event per
/// classification bucket beats N events for N failures when the
/// admin only wants the headline.
pub fn log_outcome_summary(entity: EntityKind, failures: &[ItemFailure], processed: usize) {
    use super::error::Classification::*;
    let mut transient = 0usize;
    let mut permanent = 0usize;
    let mut conflict = 0usize;
    let mut auth = 0usize;
    for f in failures {
        match f.classification() {
            Transient => transient += 1,
            Permanent => permanent += 1,
            Conflict => conflict += 1,
            Auth => auth += 1,
        }
    }
    if failures.is_empty() {
        return;
    }
    warn!(
        entity = entity.as_str(),
        processed,
        failed = failures.len(),
        // Per-bucket counts are themselves bounded numerics; the
        // labels match the classifier tag values in the per-item
        // warns so an operator can `grep classification=transient`
        // and find the matching item rows.
        transient,
        permanent,
        conflict,
        auth,
        "msgraph entity sync finished with failures"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_failure_carries_through_attempt_and_external_id() {
        let f = record_failure(
            EntityKind::Users,
            "graph-user-123",
            MsGraphSyncError::HttpPermanent {
                status: 404,
                source: Box::new(std::io::Error::new(std::io::ErrorKind::Other, "gone")),
            },
            2,
        );
        assert_eq!(f.entity, EntityKind::Users);
        assert_eq!(f.external_id, "graph-user-123");
        assert_eq!(f.attempt, 2);
        assert_eq!(f.kind_str(), "http_permanent");
    }
}
