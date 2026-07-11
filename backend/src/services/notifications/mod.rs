//! Notification Service Module
//!
//! Provides a unified notification system that fans out to multiple delivery channels.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      Ticket/Comment Handlers                     │
//! └─────────────────────────────┬───────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     NotificationService                          │
//! │  - Single entry point: notify(payload)                          │
//! │  - Checks user preferences                                       │
//! │  - Persists to database                                          │
//! │  - Fans out to enabled channels                                  │
//! └─────────────────────────────┬───────────────────────────────────┘
//!                               │
//!                     ┌───────────┴───────────┐
//!                     ▼                       ▼
//!                ┌─────────┐             ┌─────────┐
//!                │ InApp   │             │ Email   │
//!                │ Channel │             │ Channel │
//!                └─────────┘             └─────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! // Create a notification
//! let payload = NotificationPayload::new(
//!     NotificationTypeCode::TicketAssigned,
//!     recipient_uuid,
//!     NotificationActor { uuid, name, avatar_thumb },
//!     NotificationEntity::Ticket { id, title },
//!     ticket.workspace_id, // the entity's workspace; the row is pinned here
//! )
//! .with_body("You've been assigned to this ticket");
//!
//! // Send it (will check preferences and deliver to enabled channels)
//! notification_service.notify(payload).await?;
//! ```

pub mod channels;
pub mod preferences;
pub mod service;
pub mod types;

pub use service::NotificationService;
pub use types::{NotificationChannel, NotificationEvent, NotificationTypeCode};
