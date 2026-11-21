//! The rules engine: condition validation and evaluation, the event
//! consumer that runs rules with an event trigger, and the clocks and
//! worker that run time-elapsed rules. Data access stays in
//! `repository::rules` and `repository::rule_elapsed_queue`.

pub mod clocks;
pub mod conditions;
pub mod elapsed;
pub mod engine;
