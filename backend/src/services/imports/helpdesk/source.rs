//! The seam a helpdesk connector implements: fill a [`Bundle`] from the
//! source, resumably. Everything after the bundle (people, numbering, quiet
//! writes, the id map) is the loader's, shared by every source.

use std::future::Future;

use tokio_util::sync::CancellationToken;

use super::bundle::{Bundle, Manifest};

/// Counts a connector reports while it works, for the CLI and the wizard.
#[derive(Debug, Clone, Default)]
pub struct FetchProgress {
    pub people: u64,
    pub tickets: u64,
    pub comments: u64,
    pub attachments: u64,
    /// Requests the connector waited on because the source asked it to
    /// slow down (429 / Retry-After).
    pub throttled: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// The credential was refused (401/403 on the first request). Nothing
    /// was fetched.
    #[error("the source refused the credential: {0}")]
    Unauthorized(String),
    /// The source answered with something we can't read.
    #[error("unexpected answer from the source: {0}")]
    Protocol(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("bundle storage: {0}")]
    Io(#[from] std::io::Error),
    #[error("cancelled")]
    Cancelled,
}

pub trait HelpdeskSource {
    /// `zendesk`, `freshdesk`, ...: the id map's `source`.
    fn source(&self) -> &'static str;

    /// The source instance, e.g. the subdomain: the id map's `source_scope`.
    fn scope(&self) -> String;

    /// Fill `bundle`, or carry on filling it from its saved
    /// [`FetchState`](super::bundle::FetchState). Writes the manifest last,
    /// so a bundle with a manifest is complete.
    fn fetch(
        &mut self,
        bundle: &Bundle,
        progress: &(dyn Fn(&FetchProgress) + Send + Sync),
        cancel: CancellationToken,
    ) -> impl Future<Output = Result<Manifest, SourceError>> + Send;
}
