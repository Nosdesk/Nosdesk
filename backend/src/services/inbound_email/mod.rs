//! Inbound email via forwarding + AWS SES receiving (the hosted inbound path).
//!
//! A customer forwards their support mailbox to a generated
//! `<token>@inbound.<domain>` address. SES receives the mail, writes the raw
//! MIME to S3, and notifies an SNS topic; SNS POSTs the notification to the
//! webhook in `handlers::inbound_email`. The webhook verifies the SNS
//! signature ([`sns`]), fetches the raw object from S3, resolves the token to
//! a workspace + channel, and feeds the message into the existing channels
//! parse pipeline.
//!
//! Self-host keeps IMAP polling; only the ingestion source differs, the parse
//! pipeline is shared.

pub mod sns;
