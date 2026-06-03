//! Microsoft Graph delta-sync infrastructure.
//!
//! The HTTP handlers + per-entity fetch/upsert logic still live in
//! `crate::handlers::msgraph_integration` (that's the historical
//! 5k-LOC module). This module owns the *cohesive sync machinery*
//! that wraps it:
//!
//!   * [`error`] — typed `MsGraphSyncError` with thiserror, with a
//!     [`Classification`](error::Classification) the executor reads.
//!   * [`outcome`] — `EntityOutcome` / `SyncOutcome` carrying typed
//!     per-item failures, replacing the old `Vec<String>` channel.
//!   * [`retry`] — `with_retry` executor that runs an item-producing
//!     closure with exponential backoff on transient failures and a
//!     bounded attempt cap on permanent ones.
//!
//! The handler module imports these types and delegates classifying
//! its existing reqwest/diesel errors through the `From` impls; no
//! caller has to format an error as a `String` to push into a
//! result struct any more.

pub mod error;
pub mod outcome;
pub mod pipeline;
pub mod retry;

pub use error::{Classification, MsGraphSyncError, NetworkErrorKind};
pub use outcome::{EntityKind, EntityOutcome, ItemFailure, SyncOutcome};
pub use pipeline::{log_outcome_summary, record_failure};
pub use retry::{with_retry, RetryConfig};
