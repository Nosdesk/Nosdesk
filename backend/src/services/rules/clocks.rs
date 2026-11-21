//! The clocks a time-elapsed rule waits on, and when a rule on a ticket is
//! next due.
//!
//! A time-elapsed rule's `trigger_config` names a clock, a threshold in
//! minutes and an optional repeat:
//!
//! ```json
//! { "clock_name": "awaiting_requester", "minutes": 43200, "repeat_every_minutes": null }
//! ```
//!
//! A `since_*` clock (and `awaiting_requester`) fires the rule `minutes`
//! after the clock started; `until_due_at` fires it `minutes` before the
//! ticket's due date. Each clock is derived from the ticket as it is now
//! ([`ClockInputs`]), never stored: the queue (`ticket_elapsed_queue`) only
//! caches when each rule is next due, so a reconcile can rebuild it.
//!
//! A clock runs once per start: the rule fires once, then waits for the
//! clock to start again (an agent's reply restarts `since_last_agent_reply`),
//! or with a repeat, fires again every `repeat_every_minutes` after its last
//! fire. Whether a rule already fired on a ticket is read from its
//! application rows, so a rebuilt queue doesn't fire it twice.

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::models::WorkflowStateCategory;

/// The longest threshold or repeat a rule takes: a year.
pub const MAX_MINUTES: i64 = 365 * 24 * 60;
/// The shortest repeat: an hour, so a repeating rule can't email a
/// requester every minute.
pub const MIN_REPEAT_MINUTES: i64 = 60;

/// What a time-elapsed rule's clock counts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Clock {
    /// The ticket was created.
    SinceCreated,
    /// The ticket last moved to another workflow state (its creation if it
    /// never has).
    SinceStatusChange,
    /// The ticket was given its current assignee. Stopped while it has none.
    SinceAssigned,
    /// The last public reply from staff. Stopped until there is one.
    SinceLastAgentReply,
    /// The last public message from someone who isn't staff, the one that
    /// opened the ticket included. Stopped until there is one.
    SinceLastCustomerReply,
    /// The last public reply from staff, while no one else has written since:
    /// the team is waiting on the requester.
    AwaitingRequester,
    /// Counts down to the end of the ticket's due day in the workspace's
    /// timezone. Stopped while it has no due date.
    UntilDueAt,
}

impl Clock {
    pub const ALL: [Clock; 7] = [
        Clock::SinceCreated,
        Clock::SinceStatusChange,
        Clock::SinceAssigned,
        Clock::SinceLastAgentReply,
        Clock::SinceLastCustomerReply,
        Clock::AwaitingRequester,
        Clock::UntilDueAt,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SinceCreated => "since_created",
            Self::SinceStatusChange => "since_status_change",
            Self::SinceAssigned => "since_assigned",
            Self::SinceLastAgentReply => "since_last_agent_reply",
            Self::SinceLastCustomerReply => "since_last_customer_reply",
            Self::AwaitingRequester => "awaiting_requester",
            Self::UntilDueAt => "until_due_at",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == name)
    }

    /// A clock that counts down to a time rather than up from one.
    pub fn counts_down(self) -> bool {
        self == Self::UntilDueAt
    }

    /// When the clock started on a ticket, or `None` while it isn't running.
    /// For `until_due_at`, the due date it counts down to.
    pub fn started_at(self, t: &ClockInputs) -> Option<DateTime<Utc>> {
        match self {
            Self::SinceCreated => Some(t.created_at),
            Self::SinceStatusChange => Some(t.state_changed_at.unwrap_or(t.created_at)),
            Self::SinceAssigned => t
                .assignee_uuid
                .map(|_| t.assigned_at.unwrap_or(t.created_at)),
            Self::SinceLastAgentReply => t.agent_reply_at,
            Self::SinceLastCustomerReply => t.customer_reply_at,
            Self::AwaitingRequester => t
                .agent_reply_at
                .filter(|agent| t.customer_reply_at.is_none_or(|customer| customer < *agent)),
            Self::UntilDueAt => t.due_at,
        }
    }
}

/// A time-elapsed rule's trigger settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeTrigger {
    pub clock: Clock,
    pub minutes: i64,
    pub repeat_every_minutes: Option<i64>,
}

impl TimeTrigger {
    /// The settings as saved: the three keys and nothing else.
    pub fn to_config(self) -> Value {
        json!({
            "clock_name": self.clock.as_str(),
            "minutes": self.minutes,
            "repeat_every_minutes": self.repeat_every_minutes,
        })
    }
}

/// Read a time-elapsed rule's `trigger_config`, or say what's wrong with it.
pub fn parse_trigger(config: &Value) -> Result<TimeTrigger, &'static str> {
    let name = config
        .get("clock_name")
        .and_then(Value::as_str)
        .ok_or("a time-elapsed rule must name the clock it waits on")?;
    if name == "until_sla_due_at" {
        return Err("rules can't wait on the SLA due time yet");
    }
    let clock = Clock::parse(name).ok_or("a time-elapsed rule must name a known clock")?;
    let least = if clock.counts_down() { 0 } else { 1 };
    let minutes = config
        .get("minutes")
        .and_then(Value::as_i64)
        .filter(|m| (least..=MAX_MINUTES).contains(m))
        .ok_or(if clock.counts_down() {
            "minutes before the due date must be a whole number from 0 to a year"
        } else {
            "minutes must be a whole number from 1 to a year"
        })?;
    let repeat_every_minutes = match config.get("repeat_every_minutes") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_i64()
                .filter(|m| (MIN_REPEAT_MINUTES..=MAX_MINUTES).contains(m))
                .ok_or("a repeat must be a whole number of minutes from 60 to a year")?,
        ),
    };
    Ok(TimeTrigger {
        clock,
        minutes,
        repeat_every_minutes,
    })
}

/// What the clocks read from one ticket.
#[derive(Debug, Clone)]
pub struct ClockInputs {
    pub ticket_id: i32,
    pub created_at: DateTime<Utc>,
    /// When the ticket is due: the end of its due day in the workspace's
    /// timezone ([`crate::services::ticket_dates::due_instant`]).
    pub due_at: Option<DateTime<Utc>>,
    pub assignee_uuid: Option<Uuid>,
    /// The last recorded change of workflow state.
    pub state_changed_at: Option<DateTime<Utc>>,
    /// The last recorded change of assignee.
    pub assigned_at: Option<DateTime<Utc>>,
    /// The last public reply from staff that no rule wrote on its own.
    pub agent_reply_at: Option<DateTime<Utc>>,
    /// The last public message from someone who isn't staff.
    pub customer_reply_at: Option<DateTime<Utc>>,
    /// Whether the ticket's clocks run at all ([`Eligibility::runs_clocks`]).
    pub runs_clocks: bool,
}

/// What decides whether a ticket's clocks run.
#[derive(Debug, Clone, Copy)]
pub struct Eligibility<'a> {
    pub category: Option<WorkflowStateCategory>,
    pub submitted_via: Option<&'a str>,
    pub verification_state: Option<&'a str>,
    pub merged: bool,
    pub held_for_approval: bool,
}

impl Eligibility<'_> {
    /// An open ticket's clocks run. A closed, cancelled or merged ticket's
    /// don't, nor do those of history an import brought in (a CSV import,
    /// or old mail a newly connected mailbox brought in), a guest request
    /// waiting for its submitter to confirm, or a request held for approval.
    pub fn runs_clocks(&self) -> bool {
        self.category.is_some_and(|c| !c.is_terminal())
            && !crate::models::is_imported_via(self.submitted_via)
            && self.verification_state != Some(crate::sync::groups::PENDING_VERIFICATION)
            && !self.merged
            && !self.held_for_approval
    }
}

/// When a rule waits until on a ticket: its clock's start, and when it is
/// next due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wait {
    pub started_at: DateTime<Utc>,
    pub due_at: DateTime<Utc>,
}

/// When `trigger` is next due on a ticket, given when the rule last fired
/// there, or `None` when it waits for nothing: the ticket's clocks don't
/// run, the clock isn't running, or the rule already fired for this start
/// and doesn't repeat.
pub fn next_wait(
    trigger: &TimeTrigger,
    ticket: &ClockInputs,
    last_fired: Option<DateTime<Utc>>,
) -> Option<Wait> {
    if !ticket.runs_clocks {
        return None;
    }
    let started_at = trigger.clock.started_at(ticket)?;
    let threshold = Duration::minutes(trigger.minutes);
    let (due_at, period_begins) = if trigger.clock.counts_down() {
        // A due date moved later starts a new countdown; one moved earlier
        // than the last fire doesn't.
        (started_at - threshold, started_at - threshold)
    } else {
        (started_at + threshold, started_at)
    };
    match last_fired.filter(|fired| *fired >= period_begins) {
        None => Some(Wait { started_at, due_at }),
        Some(fired) => trigger.repeat_every_minutes.map(|every| Wait {
            started_at,
            due_at: fired + Duration::minutes(every),
        }),
    }
}

/// Minutes from the clock's start to `now`: how long the ticket has
/// waited, or for `until_due_at`, minus how long is left.
pub fn minutes_since(started_at: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
    (now - started_at).num_minutes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap() + Duration::minutes(minutes)
    }

    fn ticket() -> ClockInputs {
        ClockInputs {
            ticket_id: 1,
            created_at: at(0),
            due_at: None,
            assignee_uuid: None,
            state_changed_at: None,
            assigned_at: None,
            agent_reply_at: None,
            customer_reply_at: None,
            runs_clocks: true,
        }
    }

    fn trigger(clock: Clock, minutes: i64, repeat: Option<i64>) -> TimeTrigger {
        TimeTrigger {
            clock,
            minutes,
            repeat_every_minutes: repeat,
        }
    }

    #[test]
    fn settings_parse_and_normalize() {
        let t = parse_trigger(&json!({
            "clock_name": "awaiting_requester", "minutes": 60, "extra": 1
        }))
        .unwrap();
        assert_eq!(t, trigger(Clock::AwaitingRequester, 60, None));
        assert_eq!(
            t.to_config(),
            json!({ "clock_name": "awaiting_requester", "minutes": 60, "repeat_every_minutes": null })
        );
        let due = parse_trigger(&json!({ "clock_name": "until_due_at", "minutes": 0 })).unwrap();
        assert_eq!(due.minutes, 0);
        let repeat = parse_trigger(&json!({
            "clock_name": "since_created", "minutes": 5, "repeat_every_minutes": 120
        }))
        .unwrap();
        assert_eq!(repeat.repeat_every_minutes, Some(120));
    }

    #[test]
    fn settings_refuse_what_cant_run() {
        for config in [
            json!({}),
            json!({ "clock_name": "since_lunch", "minutes": 5 }),
            json!({ "clock_name": "until_sla_due_at", "minutes": 5 }),
            json!({ "clock_name": "since_created" }),
            json!({ "clock_name": "since_created", "minutes": 0 }),
            json!({ "clock_name": "since_created", "minutes": -5 }),
            json!({ "clock_name": "since_created", "minutes": "5" }),
            json!({ "clock_name": "since_created", "minutes": MAX_MINUTES + 1 }),
            json!({ "clock_name": "until_due_at", "minutes": -1 }),
            json!({ "clock_name": "since_created", "minutes": 5, "repeat_every_minutes": 59 }),
            json!({ "clock_name": "since_created", "minutes": 5, "repeat_every_minutes": "daily" }),
        ] {
            assert!(parse_trigger(&config).is_err(), "{config}");
        }
    }

    #[test]
    fn each_clock_starts_from_its_own_moment() {
        let mut t = ticket();
        assert_eq!(Clock::SinceCreated.started_at(&t), Some(at(0)));
        assert_eq!(Clock::SinceStatusChange.started_at(&t), Some(at(0)));
        t.state_changed_at = Some(at(10));
        assert_eq!(Clock::SinceStatusChange.started_at(&t), Some(at(10)));

        assert_eq!(Clock::SinceAssigned.started_at(&t), None);
        t.assignee_uuid = Some(Uuid::nil());
        assert_eq!(Clock::SinceAssigned.started_at(&t), Some(at(0)));
        t.assigned_at = Some(at(20));
        assert_eq!(Clock::SinceAssigned.started_at(&t), Some(at(20)));

        assert_eq!(Clock::UntilDueAt.started_at(&t), None);
        t.due_at = Some(at(500));
        assert_eq!(Clock::UntilDueAt.started_at(&t), Some(at(500)));
    }

    #[test]
    fn awaiting_requester_runs_from_a_staff_reply_nobody_has_answered() {
        let mut t = ticket();
        t.customer_reply_at = Some(at(0));
        assert_eq!(Clock::AwaitingRequester.started_at(&t), None);
        t.agent_reply_at = Some(at(30));
        assert_eq!(Clock::AwaitingRequester.started_at(&t), Some(at(30)));
        assert_eq!(Clock::SinceLastAgentReply.started_at(&t), Some(at(30)));
        assert_eq!(Clock::SinceLastCustomerReply.started_at(&t), Some(at(0)));
        // The requester answered: nobody is waiting on them.
        t.customer_reply_at = Some(at(40));
        assert_eq!(Clock::AwaitingRequester.started_at(&t), None);
        // Staff answered back: waiting again, from the new reply.
        t.agent_reply_at = Some(at(50));
        assert_eq!(Clock::AwaitingRequester.started_at(&t), Some(at(50)));
    }

    #[test]
    fn a_rule_is_due_once_per_start_unless_it_repeats() {
        let mut t = ticket();
        t.agent_reply_at = Some(at(100));
        let once = trigger(Clock::SinceLastAgentReply, 60, None);
        let wait = next_wait(&once, &t, None).unwrap();
        assert_eq!((wait.started_at, wait.due_at), (at(100), at(160)));
        // A fire before this start was for an earlier one.
        assert_eq!(next_wait(&once, &t, Some(at(90))), Some(wait));
        // Fired for this start: done until the clock starts again.
        assert_eq!(next_wait(&once, &t, Some(at(161))), None);
        t.agent_reply_at = Some(at(300));
        assert_eq!(next_wait(&once, &t, Some(at(161))).unwrap().due_at, at(360));

        let daily = trigger(Clock::SinceLastAgentReply, 60, Some(24 * 60));
        assert_eq!(
            next_wait(&daily, &t, Some(at(361))).unwrap().due_at,
            at(361 + 24 * 60)
        );
    }

    #[test]
    fn a_countdown_fires_before_the_due_date_and_again_when_it_moves_later() {
        let mut t = ticket();
        t.due_at = Some(at(1000));
        let day_before = trigger(Clock::UntilDueAt, 60, None);
        assert_eq!(next_wait(&day_before, &t, None).unwrap().due_at, at(940));
        assert_eq!(next_wait(&day_before, &t, Some(at(941))), None);
        t.due_at = Some(at(2000));
        assert_eq!(
            next_wait(&day_before, &t, Some(at(941))).unwrap().due_at,
            at(1940)
        );
    }

    #[test]
    fn a_ticket_whose_clocks_dont_run_waits_for_nothing() {
        let mut t = ticket();
        t.runs_clocks = false;
        assert_eq!(
            next_wait(&trigger(Clock::SinceCreated, 1, None), &t, None),
            None
        );
    }

    #[test]
    fn closed_imported_pending_merged_and_held_tickets_run_no_clocks() {
        let open = Eligibility {
            category: Some(WorkflowStateCategory::Active),
            submitted_via: Some("email_imap"),
            verification_state: None,
            merged: false,
            held_for_approval: false,
        };
        assert!(open.runs_clocks());
        for closed in [
            WorkflowStateCategory::Done,
            WorkflowStateCategory::Cancelled,
            WorkflowStateCategory::Merged,
        ] {
            assert!(!Eligibility {
                category: Some(closed),
                ..open
            }
            .runs_clocks());
        }
        for imported in ["csv_import", "email_backlog"] {
            assert!(!Eligibility {
                submitted_via: Some(imported),
                ..open
            }
            .runs_clocks());
        }
        assert!(!Eligibility {
            verification_state: Some("pending"),
            ..open
        }
        .runs_clocks());
        assert!(!Eligibility {
            merged: true,
            ..open
        }
        .runs_clocks());
        assert!(!Eligibility {
            held_for_approval: true,
            ..open
        }
        .runs_clocks());
    }
}
