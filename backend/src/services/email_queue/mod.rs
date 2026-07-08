//! Outbound email queue worker — Item J Pass 1.
//!
//! This module replaces the fire-and-forget `tokio::spawn` send path
//! in `services/channels/outbound.rs` with a durable, retryable queue.
//!
//! Architecture:
//!
//! ```text
//!   handler                                         queue worker
//!     │                                                  │
//!     │ enqueue(NewOutboundEmail) ─→ outbound_emails    │
//!     │                                  │ INSERT       │
//!     │                                  │ trigger      │
//!     │                                  ▼              │
//!     │                              pg_notify('outbound_emails_new')
//!     │                                                  │ NOTIFY
//!     │                                                  ▼
//!     │                                              listener
//!     │                                                  │ wakes
//!     │                                                  ▼
//!     │                                              claim_batch
//!     │                                                  │ FOR UPDATE SKIP LOCKED
//!     │                                                  │ → status='sending'
//!     │                                                  │ + lease (5min)
//!     │                                                  ▼
//!     │                                          dispatch via
//!     │                                          channel adapter (SMTP)
//!     │                                                  │
//!     │                                                  ▼
//!     │                                       mark_sent / mark_failed /
//!     │                                       mark_dead / mark_suppressed
//! ```
//!
//! Crash recovery: if the worker dies after marking a row `sending` but
//! before terminating it, the lease expires (5 min) and the periodic
//! lease sweeper (`sweep_expired_leases`) bumps it back to `failed` for
//! retry. Combined with the deterministic `Message-ID` persisted at
//! enqueue, this is at-least-once delivery; receiving MTAs and customer
//! MUAs deduplicate on the Message-ID.
//!
//! Files:
//! - `worker.rs` — the main loop: claim → dispatch → mark
//! - `retry.rs` — backoff + jitter + SMTP code → retry decision
//! - `circuit.rs` — circuit breaker around the SMTP transport
//!
//! Wired in `main.rs`:
//! - `services::sync_outbox`-style listener spawn (separate file once
//!   the listener-side glue lands in Pass 1.4)
//! - `services::scheduled_jobs::sweep_outbound_email_leases` periodic

pub mod circuit;
pub mod listener;
pub mod retry;
pub mod worker;

pub use listener::spawn;
pub use worker::{run_one_drain, WorkerStats};
