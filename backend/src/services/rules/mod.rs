//! The rules engine: condition validation and evaluation, and the event
//! consumer that runs rules with an event trigger. Data access stays in
//! `repository::rules`.

pub mod conditions;
pub mod engine;
