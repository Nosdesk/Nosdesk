//! Import from another helpdesk: tickets with their whole conversation,
//! people, attachments, original dates and numbers.
//!
//! Design: `docs/plans/onboarding-1.2/02-peppermint-review-and-framework.md`
//! §2. Three parts:
//!
//! 1. A connector per source ([`source::HelpdeskSource`]; Zendesk first)
//!    fills a [`bundle::Bundle`], a source-neutral copy on disk. Connectors
//!    never write to Nosdesk.
//! 2. The loader plans the bundle against the workspace (a dry run), then
//!    writes it in batches through the import write functions
//!    (`repository::tickets::import_ticket`,
//!    `repository::comments::import_comment`), recording every row in the id
//!    map (`repository::import_source_ids`) so a run resumes and a rerun
//!    skips what landed.
//! 3. Everything is written under an `import:<job>` actor reference, which
//!    the notification, rule and webhook outbox triggers skip: imported
//!    history notifies nobody. SLA is off on imported tickets.

pub mod bundle;
pub mod loader;
pub mod people;
pub mod report;
pub mod source;
pub mod zendesk;
