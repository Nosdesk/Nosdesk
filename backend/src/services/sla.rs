//! SLA engine.
//!
//! Two layers:
//!
//! 1. **Business-hours arithmetic** — `add_business_minutes(start,
//!    minutes, calendar)` walks a working calendar to add elapsed
//!    business minutes onto a wall-clock timestamp, skipping
//!    nights, weekends, and explicit holidays. The same primitive
//!    drives target-time projection ("when does this ticket
//!    breach?") and elapsed-time accumulation ("how much business
//!    time has the ticket been in active state?").
//!
//! 2. **Pill computation**: `compute_pill(ticket, clock, policy,
//!    calendar, holidays, now)` returns the spec'd CardData.sla
//!    payload `{ target_at, breached, paused, pill_color,
//!    seconds_remaining }`. What the ticket's state does to the clock
//!    is the caller's to resolve ([`StateClock`]): its category stops a
//!    finished ticket, and the state's own `pauses_sla` flag
//!    (admin-editable, defaults from the category at create time)
//!    pauses the rest.
//!
//! This module is read-only. SLA pills are derived on every read;
//! there's no separate `sla_application` row to maintain. If the
//! perf tax becomes real (~thousands of tickets per bootstrap),
//! the natural next step is to materialise pill values onto the
//! ticket row and invalidate via sync_actions on workflow_state
//! transitions.

use chrono::{
    DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc,
    Weekday,
};
use chrono_tz::Tz;
use std::collections::{HashMap, HashSet};

use crate::models::{SlaPolicy, Ticket, WorkingCalendar, WorkingCalendarHoliday};

/// One [open, close) interval inside a working day.
#[derive(Debug, Clone, Copy)]
struct WorkRange {
    open_minutes: i32, // minutes since midnight
    close_minutes: i32,
}

/// Parsed weekly schedule. `days[Weekday::Mon as usize]` etc.
#[derive(Debug, Clone)]
struct ParsedSchedule {
    days: [Vec<WorkRange>; 7],
}

fn day_index(w: Weekday) -> usize {
    // Monday=0 to match the JSON keys mon/tue/.../sun.
    w.num_days_from_monday() as usize
}

fn parse_time(s: &str) -> Option<(i32, i32)> {
    let mut parts = s.split(':');
    let h: i32 = parts.next()?.parse().ok()?;
    let m: i32 = parts.next()?.parse().ok()?;
    if !(0..=23).contains(&h) || !(0..=59).contains(&m) {
        return None;
    }
    Some((h, m))
}

/// Parse the JSONB schedule into the typed shape we walk inside
/// the arithmetic loop. Malformed entries are silently dropped so
/// a single bad range doesn't take the whole policy down — the
/// pill just becomes paused-by-empty-day until the admin fixes it.
fn parse_schedule(schedule: &serde_json::Value) -> ParsedSchedule {
    const KEYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    let mut days: [Vec<WorkRange>; 7] = Default::default();
    for (i, key) in KEYS.iter().enumerate() {
        if let Some(arr) = schedule.get(key).and_then(|v| v.as_array()) {
            for range in arr {
                let pair = range.as_array();
                if let Some(pair) = pair {
                    if pair.len() != 2 {
                        continue;
                    }
                    let open = pair[0].as_str().and_then(parse_time);
                    let close = pair[1].as_str().and_then(parse_time);
                    if let (Some((oh, om)), Some((ch, cm))) = (open, close) {
                        let open_minutes = oh * 60 + om;
                        let close_minutes = ch * 60 + cm;
                        if close_minutes > open_minutes {
                            days[i].push(WorkRange {
                                open_minutes,
                                close_minutes,
                            });
                        }
                    }
                }
            }
        }
    }
    ParsedSchedule { days }
}

fn parse_tz(tz: &str) -> Tz {
    tz.parse::<Tz>().unwrap_or(chrono_tz::UTC)
}

/// Add business minutes onto a wall-clock instant. Walks day by
/// day, consuming each day's open ranges in order. Holidays count
/// as non-working regardless of what the schedule says.
pub fn add_business_minutes(
    start: DateTime<Utc>,
    minutes: i64,
    calendar: &WorkingCalendar,
    holidays: &HashSet<NaiveDate>,
) -> DateTime<Utc> {
    let tz = parse_tz(&calendar.timezone);
    let schedule = parse_schedule(&calendar.schedule);
    let mut remaining = minutes;
    let mut local = start.with_timezone(&tz);

    // Cap iterations as a safety net; one minute per business hour
    // means a year of business hours is about 250k iterations,
    // well below this bound. If we hit it, something's wrong with
    // the schedule (every day non-working) — return the input so
    // the pill renders as "never breaches" rather than spinning.
    const MAX_DAYS: u32 = 365 * 5;

    for _ in 0..MAX_DAYS {
        if remaining <= 0 {
            return local.with_timezone(&Utc);
        }
        let date = local.date_naive();
        let weekday = date.weekday();
        let day_ranges = &schedule.days[day_index(weekday)];
        if day_ranges.is_empty() || holidays.contains(&date) {
            // Jump to start of next day.
            local = next_day_midnight(&tz, date);
            continue;
        }

        let cursor_minutes = local.hour() as i32 * 60 + local.minute() as i32;
        for range in day_ranges {
            if cursor_minutes >= range.close_minutes {
                continue;
            }
            let effective_open = cursor_minutes.max(range.open_minutes);
            let available = (range.close_minutes - effective_open) as i64;
            if available <= 0 {
                continue;
            }
            if remaining <= available {
                let target_minutes = effective_open + remaining as i32;
                let h = (target_minutes / 60) as u32;
                let m = (target_minutes % 60) as u32;
                let nd = date.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap_or_default());
                if let chrono::LocalResult::Single(dt) = tz.from_local_datetime(&nd) {
                    return dt.with_timezone(&Utc);
                }
                return local.with_timezone(&Utc);
            }
            remaining -= available;
        }
        // No range left in this day; move to start of the next.
        local = next_day_midnight(&tz, date);
    }
    // Fall-through: refuse to lie about the breach time when the
    // schedule has no working hours at all.
    start
}

/// Business minutes elapsed in `[start, end)` for a calendar — the inverse of
/// [`add_business_minutes`]. Used to advance the SLA anchor past a pause: on
/// resume we push the anchor forward by exactly the working time that elapsed
/// while paused, so paused time doesn't count against the target. Nights,
/// weekends, and holidays contribute zero.
pub fn business_minutes_between(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    calendar: &WorkingCalendar,
    holidays: &HashSet<NaiveDate>,
) -> i64 {
    if end <= start {
        return 0;
    }
    let tz = parse_tz(&calendar.timezone);
    let schedule = parse_schedule(&calendar.schedule);
    let end_local = end.with_timezone(&tz);
    let mut local = start.with_timezone(&tz);
    let mut total: i64 = 0;

    const MAX_DAYS: u32 = 365 * 5;
    for _ in 0..MAX_DAYS {
        if local >= end_local {
            break;
        }
        let date = local.date_naive();
        let day_ranges = &schedule.days[day_index(date.weekday())];
        if day_ranges.is_empty() || holidays.contains(&date) {
            local = next_day_midnight(&tz, date);
            continue;
        }
        // On the first day `local` carries the start time; on later days it's
        // midnight (cursor 0). Cap the final day at `end`'s minute.
        let cursor = local.hour() as i32 * 60 + local.minute() as i32;
        let day_cap = if end_local.date_naive() == date {
            end_local.hour() as i32 * 60 + end_local.minute() as i32
        } else {
            24 * 60
        };
        for range in day_ranges {
            let seg_start = cursor.max(range.open_minutes);
            let seg_end = range.close_minutes.min(day_cap);
            if seg_end > seg_start {
                total += (seg_end - seg_start) as i64;
            }
        }
        local = next_day_midnight(&tz, date);
    }
    total
}

fn next_day_midnight(tz: &Tz, date: NaiveDate) -> DateTime<Tz> {
    let next = date.succ_opt().unwrap_or(date);
    let nd = next.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap_or_default());
    tz.from_local_datetime(&nd).single().unwrap_or_else(|| {
        // DST gap at midnight: bump by an hour. This is rare and
        // only matters for jurisdictions whose wall clock skips
        // 00:00 once a year (none widely used).
        let bumped = nd + Duration::hours(1);
        tz.from_local_datetime(&bumped).single().unwrap_or_else(|| {
            tz.timestamp_opt(0, 0)
                .single()
                .unwrap_or_else(|| Utc::now().with_timezone(tz))
        })
    })
}

/// One SLA timer's render state. Two timers per ticket (response,
/// resolution) — see [`SlaPill`]. The field shape mirrors the v1
/// payload (`target_at`, `breached`, `paused`, `pill_color`,
/// `seconds_remaining`) so frontend pill rendering stays the same per
/// timer; the new addition is `met_at`, which the response timer sets
/// when `first_response_at` lands and the resolution timer leaves
/// `None` until we model close-vs-resolved separately.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SlaTimer {
    /// Wall-clock start of the timer — the ticket's `created_at` for
    /// both response and resolution today. Carried in the payload so
    /// the frontend can derive the at-risk threshold live (within 25%
    /// of `target_at - start_at` remaining flips to amber). Without
    /// it the at-risk transition wouldn't go live between
    /// server-emitted updates.
    pub start_at: DateTime<Utc>,
    pub target_at: DateTime<Utc>,
    /// When the timer was satisfied (e.g. `first_response_at` for the
    /// response timer). Omitted from JSON when `None` so consumers can
    /// treat absence as "still ticking".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub met_at: Option<DateTime<Utc>>,
    pub breached: bool,
    pub paused: bool,
    pub pill_color: &'static str,
    pub seconds_remaining: Option<i64>,
}

impl SlaTimer {
    /// Does this timer belong in the breach-detection scan? True when
    /// it's still ticking — neither met (response only) nor paused.
    /// The stamping helper writes `target_at = NULL` for non-scannable
    /// timers so the partial scan index naturally excludes the row.
    fn is_scannable(&self) -> bool {
        self.met_at.is_none() && !self.paused
    }
}

/// Full SLA payload for one ticket. One timer is flattened to the top
/// level for every v1 consumer (`TicketRow` pill column, the filter
/// facet, `KanbanBoard`, the calendar, the policy counts). The nested
/// `response` + `resolution` sub-objects carry each timer as it is, for
/// the preview pane to stack.
///
/// The flattened state and countdown are separate:
/// - `breached` (and `pill_color` red) is the ticket's: true when any
///   timer breached, computed or stamped by the breach job, and not met
///   in time. So the pill never disagrees with a breach notification.
/// - `target_at`, `start_at`, `met_at`, `paused` and `seconds_remaining`
///   are the timer to count down to: the unmet timer due first, else
///   the earliest breached timer, else whichever exists.
///
/// After a late response the pill is red and counts down to the
/// resolution target.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SlaPill {
    #[serde(flatten)]
    pub primary: SlaTimer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<SlaTimer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<SlaTimer>,
}

/// Compute a single timer's render state given its target window and
/// what has already happened to it. Shared by response + resolution.
fn compute_timer(
    target_minutes: i64,
    start_from: DateTime<Utc>,
    events: TimerEvents,
    paused: bool,
    calendar: &WorkingCalendar,
    holidays: &HashSet<NaiveDate>,
    now: DateTime<Utc>,
) -> SlaTimer {
    let target_at = add_business_minutes(start_from, target_minutes, calendar, holidays);

    let met_at = events.met_at;
    // A met timer is judged against when it was met, not the wall
    // clock — so a response that lands 1m before the target is "met
    // on time" even if we're observing 3h later. An unmet timer the
    // breach job stamped stays breached while the clock is paused or
    // stopped: that breach was notified. The stamp stands while the
    // target is behind us; a target moved later and still ahead means
    // the timer is running again (the recompute clears the stamp, see
    // `SlaStamp`).
    let stamped = events.breached_at.is_some() && target_at <= now;
    let breached = match met_at {
        Some(met) => met > target_at,
        None => stamped || (!paused && now > target_at),
    };

    let seconds_remaining = if met_at.is_some() {
        None
    } else if breached {
        Some((now - target_at).num_seconds().saturating_neg())
    } else {
        Some((target_at - now).num_seconds().max(0))
    };

    let pill_color = if breached {
        "red"
    } else if met_at.is_some() {
        // Met on time — the timer is done; show it as green/resolved.
        "green"
    } else if paused {
        "amber"
    } else {
        // Within 25% of the window remaining flips to amber so the
        // pill flags work that's about to breach without waiting for
        // the actual transition.
        let window_seconds = (target_at - start_from).num_seconds().max(1);
        let remaining = seconds_remaining.unwrap_or(0);
        if remaining * 4 < window_seconds {
            "amber"
        } else {
            "green"
        }
    };

    SlaTimer {
        start_at: start_from,
        target_at,
        met_at,
        breached,
        paused,
        pill_color,
        seconds_remaining,
    }
}

/// What has already happened to one timer.
#[derive(Debug, Clone, Copy, Default)]
struct TimerEvents {
    /// When it was satisfied: `first_response_at` for the response
    /// timer; the resolution timer is never met.
    met_at: Option<DateTime<Utc>>,
    /// When the breach job stamped it breached (`sla_*_breached_at`).
    breached_at: Option<DateTime<Utc>>,
}

/// When a policy's SLA clock starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockStart {
    /// From ticket creation; the clock runs continuously (the workflow pause is
    /// ignored — the promise is measured from submission).
    Created,
    /// From the ticket's first entry into a non-pausing (Active) state; pause
    /// then subtracts. Until that first activation the clock is `not_started`.
    Activated,
}

impl ClockStart {
    /// Parse the policy's stored string; anything unrecognised (incl. legacy
    /// rows) falls back to `Activated`, the bug-fixing default.
    pub fn parse(s: &str) -> Self {
        match s {
            "created" => ClockStart::Created,
            _ => ClockStart::Activated,
        }
    }
}

/// What a ticket's workflow state does to its SLA clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateClock {
    /// The state counts time.
    Running,
    /// The state's `pauses_sla` flag is on: an `activated` clock freezes, a
    /// `created` clock keeps running.
    Paused,
    /// The ticket is finished (Done, Cancelled or Merged): every clock stops,
    /// so nothing it does any more can breach.
    Stopped,
}

impl StateClock {
    /// The clock effect of `state`.
    pub fn of(state: &crate::models::WorkflowState) -> Self {
        if state.category.is_terminal() {
            Self::Stopped
        } else if state.pauses_sla {
            Self::Paused
        } else {
            Self::Running
        }
    }

    /// The clock effect of the state `workflow_state_id`. A missing row counts
    /// as paused, so an unresolvable state never starts counting time.
    pub fn of_state_id(conn: &mut crate::db::DbConnection, workflow_state_id: i32) -> Self {
        use crate::schema::workflow_states;
        use diesel::prelude::*;
        workflow_states::table
            .find(workflow_state_id)
            .first::<crate::models::WorkflowState>(conn)
            .map(|state| Self::of(&state))
            .unwrap_or(Self::Paused)
    }
}

/// Compute the SLA pill payload for a ticket — both response +
/// resolution timers, gated on which policy targets are configured.
/// `clock` is what the ticket's workflow state does to the clock (see
/// [`StateClock`]). The response timer also stops counting once
/// `first_response_at` is stamped, regardless of pause state — at
/// that point the response was either met or breached, and the wall
/// clock has nothing left to say about it.
///
/// Returns `None` when the policy has neither a response target nor a
/// resolution target (no pill to render). Otherwise returns at least
/// one timer; the other is `None` when its target isn't configured.
pub fn compute_pill(
    ticket: &Ticket,
    clock: StateClock,
    policy: &SlaPolicy,
    calendar: &WorkingCalendar,
    holidays: &HashSet<NaiveDate>,
    now: DateTime<Utc>,
) -> Option<SlaPill> {
    // A per-ticket override to `none` wins over any policy: no SLA. Checked here
    // (the single point every read path routes through) so it also clears the
    // materialised targets.
    if ticket.sla_override == "none" {
        return None;
    }

    // A No-SLA policy that wins matching means this ticket has no SLA: no pill.
    // Both the bootstrap read and the recompute path route through here, so this
    // one check covers every surface (and clears the materialised targets, since
    // callers treat `None` as "no SLA").
    if policy.no_sla {
        return None;
    }

    let utc = |t: NaiveDateTime| DateTime::<Utc>::from_naive_utc_and_offset(t, Utc);
    let created_utc = utc(ticket.created_at);
    let paused = clock != StateClock::Running;

    // Resolve the effective anchor (where the clock counts from) and whether it's
    // currently frozen, per the policy's clock-start mode. This is the P2 fix for
    // the created_at-anchored instant-breach.
    let (anchor, effective_paused) = match ClockStart::parse(&policy.clock_start) {
        // Runs continuously from submission; the workflow pause doesn't apply
        // (the promise is measured from when the client contacted us). Only a
        // finished ticket stops it.
        ClockStart::Created => (created_utc, clock == StateClock::Stopped),
        ClockStart::Activated => match ticket.sla_clock_started_at {
            // Started: the anchor already absorbed prior paused time (pushed
            // forward on each resume). Honour the current workflow pause — it
            // freezes the timer now and is subtracted on the next resume.
            Some(started) => (
                DateTime::<Utc>::from_naive_utc_and_offset(started, Utc),
                paused,
            ),
            // No anchor yet. A currently-active ticket predates the stamp (a
            // pre-migration row) — fall back to created_at so it keeps showing an
            // SLA and self-heals on its next transition. Currently paused
            // (backlog/triage) means the clock genuinely never started: no pill.
            None if !paused => (created_utc, false),
            None => return None,
        },
    };

    let response = policy
        .target_response_minutes
        .filter(|m| *m > 0)
        .map(|minutes| {
            compute_timer(
                minutes as i64,
                anchor,
                TimerEvents {
                    met_at: ticket.first_response_at.map(utc),
                    breached_at: ticket.sla_response_breached_at.map(utc),
                },
                effective_paused,
                calendar,
                holidays,
                now,
            )
        });

    let resolution = policy
        .target_resolution_minutes
        .filter(|m| *m > 0)
        .map(|minutes| {
            compute_timer(
                minutes as i64,
                anchor,
                TimerEvents {
                    met_at: None,
                    breached_at: ticket.sla_resolution_breached_at.map(utc),
                },
                effective_paused,
                calendar,
                holidays,
                now,
            )
        });

    let primary = flattened_timer(response.as_ref(), resolution.as_ref())?;

    Some(SlaPill {
        primary,
        response,
        resolution,
    })
}

/// The pill's flattened timer (see [`SlaPill`]). Its countdown fields are
/// the unmet timer due first, else (nothing left to meet) the earliest
/// breached timer, else whichever exists (a response met on time with no
/// resolution target). Its `breached` is the ticket's: true when any timer
/// breached, with `pill_color` red to match.
fn flattened_timer(response: Option<&SlaTimer>, resolution: Option<&SlaTimer>) -> Option<SlaTimer> {
    let timers = || response.into_iter().chain(resolution);
    let mut flat = timers()
        .filter(|t| t.met_at.is_none())
        .min_by_key(|t| t.target_at)
        .or_else(|| timers().filter(|t| t.breached).min_by_key(|t| t.target_at))
        .or_else(|| timers().next())?
        .clone();
    if timers().any(|t| t.breached) {
        flat.breached = true;
        flat.pill_color = "red";
    }
    Some(flat)
}

/// One ticket's SLA pill as it stands, for a sync row. Reads only: the
/// clock anchor and the materialised targets are left to
/// `recompute_and_stamp_sla_for_ticket`. `Value::Null` when no policy
/// matches, which the frontend reads as "no SLA" and hides the pill.
pub fn pill_json_for_ticket(
    conn: &mut crate::db::DbConnection,
    ticket: &Ticket,
) -> serde_json::Value {
    load_pill_for_ticket(conn, ticket)
        .and_then(|p| serde_json::to_value(p).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// Recompute one ticket's SLA pill and persist the materialised
/// target timestamps in the same call. Used by mutation paths
/// (status / priority / category change, first-response stamp, the
/// breach-detection sweep) so the `sla_response_target_at` /
/// `sla_resolution_target_at` columns the breach job scans against
/// stay in lockstep with the JSON pill the frontend renders. Returns
/// the pill JSON to slot into the `ticket.sla_updated` sync_action;
/// `Value::Null` when no policy applies (and the materialised columns
/// are cleared so the breach scan ignores the row). A finished ticket's
/// clock is stopped, so its columns are cleared too.
///
/// Use this when the write didn't move the ticket into its current
/// workflow state: a running ticket with no stored clock start has then
/// been running since it was opened, and that time is stored as its start
/// (what its pill already counted from). After a state change, use
/// [`recompute_and_stamp_sla_after_state_change`].
pub fn recompute_and_stamp_sla_for_ticket(
    conn: &mut crate::db::DbConnection,
    ticket: &Ticket,
) -> serde_json::Value {
    recompute_and_stamp(conn, ticket, FirstStart::WhenOpened)
}

/// [`recompute_and_stamp_sla_for_ticket`] after a write that moved the
/// ticket from `previous_state_id`. A ticket that only now starts running
/// starts its clock now; one that was already running keeps the rule
/// above.
pub fn recompute_and_stamp_sla_after_state_change(
    conn: &mut crate::db::DbConnection,
    ticket: &Ticket,
    previous_state_id: i32,
) -> serde_json::Value {
    let first_start = if StateClock::of_state_id(conn, previous_state_id) == StateClock::Running {
        FirstStart::WhenOpened
    } else {
        FirstStart::Now
    };
    recompute_and_stamp(conn, ticket, first_start)
}

/// [`recompute_and_stamp_sla_for_ticket`] for a guest ticket just released
/// by its confirmation: it joins the workspace now, so its clock starts now.
pub fn recompute_and_stamp_sla_on_release(
    conn: &mut crate::db::DbConnection,
    ticket: &Ticket,
) -> serde_json::Value {
    recompute_and_stamp(conn, ticket, FirstStart::Now)
}

/// When a running ticket's clock started, if it has no stored start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstStart {
    /// It has been running since it was opened.
    WhenOpened,
    /// It starts running with this write.
    Now,
}

fn recompute_and_stamp(
    conn: &mut crate::db::DbConnection,
    ticket: &Ticket,
    first_start: FirstStart,
) -> serde_json::Value {
    // Advance the activation-clock state machine first (stamp the anchor on first
    // activation, record a pause start, or push the anchor past a finished pause)
    // so the pill below reflects the fresh anchor. Runs under the caller's actor
    // context, so the audited write on the ticket carries workspace context.
    // A guest ticket waiting for its email to be confirmed is outside the
    // workspace until it's released, so it has no SLA yet: no clock and no
    // targets for the breach sweep. Releasing it stamps it.
    if is_pending_verification(ticket) {
        if let Err(e) = write_sla_stamp(conn, ticket.id, &SlaStamp::default()) {
            tracing::warn!(ticket_id = ticket.id, error = %e, "clearing SLA targets failed");
        }
        return serde_json::Value::Null;
    }
    let mut ticket = ticket.clone();
    advance_sla_clock(conn, &mut ticket, first_start);

    let pill = load_pill_for_ticket(conn, &ticket);
    let clock = StateClock::of_state_id(conn, ticket.workflow_state_id);
    if let Err(e) = write_sla_stamp(
        conn,
        ticket.id,
        &SlaStamp::from_pill(pill.as_ref(), clock, Utc::now()),
    ) {
        tracing::warn!(ticket_id = ticket.id, error = %e, "stamping SLA targets failed");
    }
    pill.and_then(|p| serde_json::to_value(p).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// Whether `ticket` is a guest submission still waiting for its email to be
/// confirmed. It has no SLA until then.
pub fn is_pending_verification(ticket: &Ticket) -> bool {
    ticket.verification_state.as_deref() == Some(crate::sync::groups::PENDING_VERIFICATION)
}

/// Advance one ticket's SLA clock after a workflow transition. Only
/// `activated`-clock policies use the anchor; a `created` / No-SLA / unmatched
/// ticket is left untouched. Mutates the ticket's anchor fields in memory to
/// match the persisted write so the caller's pill computation sees the fresh
/// values. State machine over `(sla_clock_started_at, sla_paused_at)`:
///   - no anchor + now active        -> stamp the anchor (first activation): the
///                                       time it was opened, or now when this
///                                       write is what starts it ([`FirstStart`])
///   - anchor, running + now paused   -> record the pause start
///   - anchor, paused + now active    -> push the anchor past the paused business
///                                       time (pausing subtracts) and clear it
fn advance_sla_clock(
    conn: &mut crate::db::DbConnection,
    ticket: &mut Ticket,
    first_start: FirstStart,
) {
    use crate::schema::{sla_policies, tickets};
    use diesel::prelude::*;

    // An overridden-off ticket has no SLA — nothing to anchor.
    if ticket.sla_override == "none" {
        return;
    }
    let Ok(policies) = sla_policies::table.load::<SlaPolicy>(conn) else {
        return;
    };
    let group_ids = ticket
        .assignee_uuid
        .and_then(|u| crate::repository::groups::get_group_ids_for_user(conn, &u).ok())
        .unwrap_or_default();
    let Some(policy) = pick_policy(&policies, ticket, &group_ids) else {
        return;
    };
    if policy.no_sla || ClockStart::parse(&policy.clock_start) != ClockStart::Activated {
        return;
    }

    // A finished ticket holds the clock like a paused one, so reopening it
    // doesn't count the time it spent closed.
    let now_paused = StateClock::of_state_id(conn, ticket.workflow_state_id) != StateClock::Running;
    let now = Utc::now();

    let (new_anchor, new_paused_at): (Option<NaiveDateTime>, Option<NaiveDateTime>) =
        match (ticket.sla_clock_started_at, ticket.sla_paused_at) {
            (None, _) if now_paused => (None, None), // still not started
            (None, _) => (
                Some(match first_start {
                    FirstStart::WhenOpened => ticket.created_at,
                    FirstStart::Now => now.naive_utc(),
                }),
                None,
            ), // first activation
            (Some(anchor), None) if now_paused => (Some(anchor), Some(now.naive_utc())), // pause
            (Some(anchor), None) => (Some(anchor), None), // running
            (Some(anchor), Some(paused_at)) if now_paused => (Some(anchor), Some(paused_at)), // held
            (Some(anchor), Some(paused_at)) => {
                // Resume: advance the anchor past the paused working time.
                (
                    Some(push_anchor_past_pause(conn, policy, anchor, paused_at, now)),
                    None,
                )
            }
        };

    if new_anchor != ticket.sla_clock_started_at || new_paused_at != ticket.sla_paused_at {
        let _ = diesel::update(tickets::table.find(ticket.id))
            .set((
                tickets::sla_clock_started_at.eq(new_anchor),
                tickets::sla_paused_at.eq(new_paused_at),
            ))
            .execute(conn);
        ticket.sla_clock_started_at = new_anchor;
        ticket.sla_paused_at = new_paused_at;
    }
}

/// Push the SLA anchor forward by the business time that elapsed during a pause,
/// so the paused window doesn't count toward the target. Falls back to the
/// unchanged anchor if the policy has no calendar to measure against.
fn push_anchor_past_pause(
    conn: &mut crate::db::DbConnection,
    policy: &SlaPolicy,
    anchor: NaiveDateTime,
    paused_at: NaiveDateTime,
    now: DateTime<Utc>,
) -> NaiveDateTime {
    let Some((calendar, holidays)) = load_calendar_for_policy(conn, policy) else {
        return anchor;
    };

    let paused_from = DateTime::<Utc>::from_naive_utc_and_offset(paused_at, Utc);
    let paused_business = business_minutes_between(paused_from, now, &calendar, &holidays);
    let anchor_utc = DateTime::<Utc>::from_naive_utc_and_offset(anchor, Utc);
    add_business_minutes(anchor_utc, paused_business, &calendar, &holidays).naive_utc()
}

/// The calendar a policy's targets are measured on. A policy without one of
/// its own (none was chosen, or its calendar was deleted, which clears the
/// link) uses its workspace's default calendar, else the workspace's first.
/// `None` only when the workspace has no calendar at all. Before, such a
/// policy switched SLA off for every ticket it matched.
pub fn calendar_for_policy<'a>(
    policy: &SlaPolicy,
    calendars: &'a HashMap<i32, WorkingCalendar>,
) -> Option<&'a WorkingCalendar> {
    if let Some(own) = policy.working_calendar_id.and_then(|id| calendars.get(&id)) {
        return Some(own);
    }
    let mut same_workspace = calendars
        .values()
        .filter(|c| c.workspace_id == policy.workspace_id);
    let first = same_workspace.clone().min_by_key(|c| c.id);
    same_workspace.find(|c| c.is_default).or(first)
}

/// [`calendar_for_policy`] read from the database, with the chosen
/// calendar's holidays expanded into dates.
pub fn load_calendar_for_policy(
    conn: &mut crate::db::DbConnection,
    policy: &SlaPolicy,
) -> Option<(WorkingCalendar, HashSet<NaiveDate>)> {
    use crate::schema::{working_calendar_holidays, working_calendars};
    use diesel::prelude::*;

    let calendars: HashMap<i32, WorkingCalendar> = working_calendars::table
        .filter(working_calendars::workspace_id.eq(policy.workspace_id))
        .load::<WorkingCalendar>(conn)
        .ok()?
        .into_iter()
        .map(|c| (c.id, c))
        .collect();
    let calendar = calendar_for_policy(policy, &calendars)?.clone();
    // Pull the full rows so annual-recurrence holidays expand into
    // their concrete dates for the year window the engine touches.
    // expand_holiday lives in the repository so the bootstrap path
    // and this per-ticket path share the same rule.
    let holiday_rows: Vec<WorkingCalendarHoliday> = working_calendar_holidays::table
        .filter(working_calendar_holidays::calendar_id.eq(calendar.id))
        .load(conn)
        .unwrap_or_default();
    let current_year = Utc::now().year();
    let holidays: HashSet<NaiveDate> = holiday_rows
        .iter()
        .flat_map(|h| crate::repository::sla::expand_holiday(h, current_year))
        .collect();
    Some((calendar, holidays))
}

/// Recompute the SLA of every open ticket in `workspace_id` after one of
/// its SLA policies, calendars or holidays changes, so existing tickets
/// pick up new, changed or removed targets (the breach sweep only scans
/// stamped targets). Runs in the caller's transaction:
///
/// - policies, calendars and holidays are read once, and the tickets are
///   locked in id order, so overlapping saves can't deadlock;
/// - a guest ticket still waiting for confirmation is skipped;
/// - a running ticket whose clock never started gets the time it was
///   opened as its start, stored once, so a later save never moves it;
/// - only rows whose SLA changes are written, and those are sent to
///   clients as `ticket.sla_updated` (a target going away included).
///
/// Returns how many tickets changed.
pub fn restamp_open_tickets(
    conn: &mut crate::db::DbConnection,
    workspace_id: i32,
) -> diesel::QueryResult<usize> {
    use crate::schema::{tickets, workflow_states};
    use diesel::prelude::*;

    let mut ctx = crate::repository::sla::load_for_pill_computation(conn)?;
    ctx.policies.retain(|p| p.workspace_id == workspace_id);
    let clocks: HashMap<i32, StateClock> = workflow_states::table
        .filter(workflow_states::workspace_id.eq(workspace_id))
        .load::<crate::models::WorkflowState>(conn)?
        .iter()
        .map(|s| (s.id, StateClock::of(s)))
        .collect();
    let open_states: Vec<i32> = clocks
        .iter()
        .filter(|(_, clock)| **clock != StateClock::Stopped)
        .map(|(id, _)| *id)
        .collect();
    let open: Vec<Ticket> = tickets::table
        .filter(tickets::workspace_id.eq(workspace_id))
        .filter(tickets::workflow_state_id.eq_any(&open_states))
        .filter(
            tickets::verification_state.is_distinct_from(crate::sync::groups::PENDING_VERIFICATION),
        )
        .order(tickets::id.asc())
        .for_no_key_update()
        .load(conn)?;

    let no_holidays = HashSet::new();
    let mut groups_by_assignee: HashMap<uuid::Uuid, Vec<i32>> = HashMap::new();
    let now = Utc::now();
    let mut moved = 0;
    for mut ticket in open {
        let clock = clocks
            .get(&ticket.workflow_state_id)
            .copied()
            .unwrap_or(StateClock::Paused);
        let group_ids = match ticket.assignee_uuid {
            Some(assignee) => groups_by_assignee
                .entry(assignee)
                .or_insert_with(|| {
                    crate::repository::groups::get_group_ids_for_user(conn, &assignee)
                        .unwrap_or_default()
                })
                .clone(),
            None => Vec::new(),
        };
        let Some(policy) = pick_policy(&ctx.policies, &ticket, &group_ids) else {
            if write_sla_stamp(conn, ticket.id, &SlaStamp::default())? {
                moved += 1;
                emit_sla_updated(conn, &ticket, serde_json::Value::Null)?;
            }
            continue;
        };

        let mut changed = false;
        // A ticket already running under a policy that counts from activation,
        // with no stored start: it has been running since it was opened (the
        // pill already counts from there). Store that once.
        if ticket.sla_override != "none"
            && !policy.no_sla
            && ClockStart::parse(&policy.clock_start) == ClockStart::Activated
            && clock == StateClock::Running
            && ticket.sla_clock_started_at.is_none()
        {
            changed |= diesel::update(tickets::table.find(ticket.id))
                .filter(tickets::sla_clock_started_at.is_null())
                .set(tickets::sla_clock_started_at.eq(ticket.created_at))
                .execute(conn)?
                > 0;
            ticket.sla_clock_started_at = Some(ticket.created_at);
        }

        let pill = calendar_for_policy(policy, &ctx.calendars_by_id).and_then(|calendar| {
            let holidays = ctx
                .holidays_by_calendar
                .get(&calendar.id)
                .unwrap_or(&no_holidays);
            compute_pill(&ticket, clock, policy, calendar, holidays, now)
        });
        changed |= write_sla_stamp(
            conn,
            ticket.id,
            &SlaStamp::from_pill(pill.as_ref(), clock, Utc::now()),
        )?;
        if changed {
            moved += 1;
            let sla = pill
                .and_then(|p| serde_json::to_value(p).ok())
                .unwrap_or(serde_json::Value::Null);
            emit_sla_updated(conn, &ticket, sla)?;
        }
    }
    Ok(moved)
}

/// Send a ticket's new SLA pill to the clients that can see the ticket.
fn emit_sla_updated(
    conn: &mut crate::db::DbConnection,
    ticket: &Ticket,
    sla: serde_json::Value,
) -> diesel::QueryResult<()> {
    use crate::sync::emit::{self, SyncEmit};
    let groups = crate::sync::groups::for_ticket(conn, ticket)?;
    emit::record(
        conn,
        SyncEmit {
            aggregate: crate::models::SyncAggregate::Ticket,
            aggregate_id: ticket.id.to_string(),
            op: crate::models::SyncOp::Update,
            event_type: "ticket.sla_updated",
            data: serde_json::json!({ "id": ticket.id, "sla": sla }),
            groups,
            causation_id: None,
        },
    )?;
    Ok(())
}

/// Load every input the engine needs for one ticket and run
/// `compute_pill`. Returns `None` when any link in the chain is
/// missing (no matching policy, no calendar in the workspace, no
/// configured targets) — callers either return null JSON or clear the
/// materialised columns accordingly.
fn load_pill_for_ticket(conn: &mut crate::db::DbConnection, ticket: &Ticket) -> Option<SlaPill> {
    use crate::schema::sla_policies;
    use diesel::prelude::*;

    let policies: Vec<SlaPolicy> = sla_policies::table.load(conn).ok()?;
    let group_ids = ticket
        .assignee_uuid
        .and_then(|u| crate::repository::groups::get_group_ids_for_user(conn, &u).ok())
        .unwrap_or_default();
    let policy = pick_policy(&policies, ticket, &group_ids)?;
    let (calendar, holidays) = load_calendar_for_policy(conn, policy)?;
    let clock = StateClock::of_state_id(conn, ticket.workflow_state_id);
    compute_pill(ticket, clock, policy, &calendar, &holidays, Utc::now())
}

/// Derive the (response, resolution) target timestamps to materialise
/// from a computed pill. Each timer contributes its `target_at` only
/// when it's still scannable (not met, not paused) — the partial scan
/// index excludes NULL rows naturally, so the breach job doesn't even
/// look at met/paused timers.
fn targets_from_pill(
    pill: &SlaPill,
) -> (Option<chrono::NaiveDateTime>, Option<chrono::NaiveDateTime>) {
    let scannable = |t: &SlaTimer| t.is_scannable().then(|| t.target_at.naive_utc());
    (
        pill.response.as_ref().and_then(scannable),
        pill.resolution.as_ref().and_then(scannable),
    )
}

/// What a recompute writes on a ticket besides its clock: the materialised
/// targets the breach sweep scans (`None` clears one), and, per timer, the
/// target a stored breach must precede to still stand.
#[derive(Debug, Default)]
struct SlaStamp {
    response_target: Option<NaiveDateTime>,
    resolution_target: Option<NaiveDateTime>,
    /// A stored breach earlier than this target no longer stands: a longer
    /// target, or time paused since, moved it later, and it is still ahead.
    /// Clearing it lets a later real breach notify. Only set for an open
    /// ticket whose target is still ahead: when the clock is already past
    /// the moved target the ticket never stopped being breached, so the
    /// stamp stays and the breach isn't notified twice. A finished ticket
    /// keeps the breaches it earned.
    response_breach_before: Option<NaiveDateTime>,
    resolution_breach_before: Option<NaiveDateTime>,
}

impl SlaStamp {
    fn from_pill(pill: Option<&SlaPill>, clock: StateClock, now: DateTime<Utc>) -> Self {
        let (response_target, resolution_target) =
            pill.map(targets_from_pill).unwrap_or((None, None));
        let open = clock != StateClock::Stopped;
        let breach_before = |timer: Option<&SlaTimer>| {
            timer
                .filter(|t| open && t.target_at > now)
                .map(|t| t.target_at.naive_utc())
        };
        Self {
            response_target,
            resolution_target,
            response_breach_before: breach_before(pill.and_then(|p| p.response.as_ref())),
            resolution_breach_before: breach_before(pill.and_then(|p| p.resolution.as_ref())),
        }
    }
}

/// Write a [`SlaStamp`]. Each statement only matches the row when it
/// changes something, so a ticket whose SLA didn't move isn't written: no
/// audit row, no dead tuple, no row lock. Returns whether the row changed.
fn write_sla_stamp(
    conn: &mut crate::db::DbConnection,
    ticket_id: i32,
    stamp: &SlaStamp,
) -> diesel::QueryResult<bool> {
    use crate::schema::tickets;
    use diesel::prelude::*;

    let mut changed = diesel::update(tickets::table.find(ticket_id))
        .filter(
            tickets::sla_response_target_at
                .is_distinct_from(stamp.response_target)
                .or(tickets::sla_resolution_target_at.is_distinct_from(stamp.resolution_target)),
        )
        .set((
            tickets::sla_response_target_at.eq(stamp.response_target),
            tickets::sla_resolution_target_at.eq(stamp.resolution_target),
        ))
        .execute(conn)?
        > 0;
    if let Some(target) = stamp.response_breach_before {
        changed |= diesel::update(tickets::table.find(ticket_id))
            .filter(tickets::sla_response_breached_at.lt(target))
            .set(tickets::sla_response_breached_at.eq(None::<NaiveDateTime>))
            .execute(conn)?
            > 0;
    }
    if let Some(target) = stamp.resolution_breach_before {
        changed |= diesel::update(tickets::table.find(ticket_id))
            .filter(tickets::sla_resolution_breached_at.lt(target))
            .set(tickets::sla_resolution_breached_at.eq(None::<NaiveDateTime>))
            .execute(conn)?
            > 0;
    }
    Ok(changed)
}

/// Pick the most-specific policy that matches a ticket. Highest-id
/// match wins, with the workspace default as a fallback.
///
/// `assignee_group_ids` lists the groups the ticket's current assignee
/// belongs to (empty when unassigned or when the assignee is in no
/// groups). A policy with `assignee_group_id_filter = NULL` matches
/// regardless; a policy with a set filter only matches when that group
/// id appears in the slice. Group resolution is left to the caller so
/// `pick_policy` stays a pure function with no DB dependency.
pub fn pick_policy<'a>(
    policies: &'a [SlaPolicy],
    ticket: &Ticket,
    assignee_group_ids: &[i32],
) -> Option<&'a SlaPolicy> {
    let mut best: Option<&SlaPolicy> = None;
    for policy in policies {
        // Saving checks the filter names a priority. One stored before that
        // which names none matches no ticket, as it always did, and is logged.
        let priority_ok = match policy.priority_filter.as_deref() {
            None => true,
            Some(filter) => match crate::models::TicketPriority::parse(filter) {
                Some(wanted) => wanted == ticket.priority,
                None => {
                    tracing::debug!(
                        policy_id = policy.id,
                        "SLA policy's priority filter names no priority; it matches no ticket"
                    );
                    false
                }
            },
        };
        let category_ok = policy
            .category_id_filter
            .map(|c| Some(c) == ticket.category_id)
            .unwrap_or(true);
        let group_ok = policy
            .assignee_group_id_filter
            .map(|g| assignee_group_ids.contains(&g))
            .unwrap_or(true);
        if !priority_ok || !category_ok || !group_ok {
            continue;
        }
        // Prefer non-default policies (more specific) over the
        // catch-all; among non-defaults, highest id wins.
        let pick = match best {
            None => true,
            Some(prev) => {
                if prev.is_default && !policy.is_default {
                    true
                } else if !prev.is_default && policy.is_default {
                    false
                } else {
                    policy.id > prev.id
                }
            }
        };
        if pick {
            best = Some(policy);
        }
    }
    best
}

// ---------------- Open-ticket scan ----------------

/// Per-policy state breakdown of currently-open tickets. Lives in
/// services because both the admin per-policy endpoint and the
/// workspace-wide dashboard widget need the same scan + bucketing.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct PolicyMatchCounts {
    pub total: i64,
    pub on_track: i64,
    pub at_risk: i64,
    pub breached: i64,
    pub paused: i64,
}

impl PolicyMatchCounts {
    /// Increment `total` and bucket the ticket by its pill state.
    /// `breached` is the ticket's (any timer breached, see
    /// [`SlaPill`]), so a ticket counts as breached exactly when the
    /// pill shows it, and breached wins over paused and at-risk.
    /// Paused and at-risk go by the timer the pill counts down to.
    fn add_pill(&mut self, pill: Option<&SlaPill>) {
        self.total += 1;
        match pill {
            Some(p) if p.primary.breached => self.breached += 1,
            Some(p) if p.primary.paused => self.paused += 1,
            Some(p) if p.primary.pill_color == "amber" => self.at_risk += 1,
            Some(_) => self.on_track += 1,
            None => self.on_track += 1,
        }
    }
}

/// Output of `scan_open_ticket_buckets`. Carries both the per-policy
/// breakdown the admin policy list uses and the workspace roll-up
/// the dashboard health widget shows.
#[derive(Debug, Default, serde::Serialize)]
pub struct OpenTicketScan {
    pub by_policy: HashMap<i32, PolicyMatchCounts>,
    pub workspace_total: PolicyMatchCounts,
}

/// One pass over the workspace's open tickets, bucketing each by
/// the matched policy's pill state. Reused by the admin per-policy
/// endpoint and the dashboard workspace-summary endpoint — same
/// scan, two aggregations.
///
/// The scan is capped at `limit`; counts stay useful as
/// approximations above the cap. Materialised counts would be the
/// next step if a real workspace routinely hits it.
pub fn scan_open_ticket_buckets(
    conn: &mut crate::db::DbConnection,
    limit: i64,
) -> diesel::QueryResult<OpenTicketScan> {
    use crate::models::WorkflowStateCategory;
    use crate::schema::{tickets, workflow_states};
    use diesel::prelude::*;

    let ctx = crate::repository::sla::load_for_pill_computation(conn)?;

    // Open = not in a terminal category (Done, Cancelled, Merged). Two
    // cheap queries: pick the open state ids + their pauses_sla flag, then
    // load tickets in those states. Ticket doesn't derive Selectable so we
    // avoid the inner-join select tuple.
    let open_states: Vec<(i32, bool)> = workflow_states::table
        .filter(workflow_states::category.ne(WorkflowStateCategory::Done))
        .filter(workflow_states::category.ne(WorkflowStateCategory::Cancelled))
        .filter(workflow_states::category.ne(WorkflowStateCategory::Merged))
        .select((workflow_states::id, workflow_states::pauses_sla))
        .load(conn)?;
    let open_state_ids: Vec<i32> = open_states.iter().map(|(id, _)| *id).collect();
    let pause_by_state: HashMap<i32, bool> = open_states.into_iter().collect();

    let open_tickets: Vec<Ticket> = tickets::table
        .filter(tickets::workflow_state_id.eq_any(&open_state_ids))
        .limit(limit)
        .load(conn)?;

    // Batch-load assignee group memberships so the matcher can honour
    // assignee_group_id_filter without N+1.
    let assignee_uuids: Vec<uuid::Uuid> = open_tickets
        .iter()
        .filter_map(|t| t.assignee_uuid)
        .collect();
    let groups_by_assignee =
        crate::repository::groups::get_group_ids_for_users(conn, &assignee_uuids)
            .unwrap_or_default();

    let now = Utc::now();
    let mut by_policy: HashMap<i32, PolicyMatchCounts> = HashMap::new();
    let mut workspace_total = PolicyMatchCounts::default();

    for ticket in open_tickets {
        // Default to paused so a state missing from the lookup
        // (race during a delete) doesn't accidentally start
        // counting a stale ticket.
        let clock = match pause_by_state.get(&ticket.workflow_state_id) {
            Some(false) => StateClock::Running,
            _ => StateClock::Paused,
        };
        let assignee_groups = ticket
            .assignee_uuid
            .and_then(|u| groups_by_assignee.get(&u))
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        // A per-ticket override to `none` has no SLA — exclude from the counts.
        if ticket.sla_override == "none" {
            continue;
        }
        let Some(policy) = pick_policy(&ctx.policies, &ticket, assignee_groups) else {
            continue;
        };
        // A No-SLA policy means this ticket has no SLA — exclude it from the SLA
        // health counts rather than defaulting it into on_track / total.
        if policy.no_sla {
            continue;
        }

        // No calendar in the workspace -> no pill; the policy still
        // matches the ticket so it counts toward `total` but lands in
        // `on_track` as a neutral default.
        let pill = calendar_for_policy(policy, &ctx.calendars_by_id).and_then(|calendar| {
            let holidays = ctx
                .holidays_by_calendar
                .get(&calendar.id)
                .cloned()
                .unwrap_or_default();
            compute_pill(&ticket, clock, policy, calendar, &holidays, now)
        });

        by_policy
            .entry(policy.id)
            .or_default()
            .add_pill(pill.as_ref());
        workspace_total.add_pill(pill.as_ref());
    }

    Ok(OpenTicketScan {
        by_policy,
        workspace_total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn cal(schedule: serde_json::Value) -> WorkingCalendar {
        WorkingCalendar {
            id: 1,
            name: "test".into(),
            timezone: "UTC".into(),
            schedule,
            is_default: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: None,
            workspace_id: 1,
        }
    }

    #[test]
    fn add_business_minutes_skips_weekend() {
        // Friday 16:00 UTC + 4 business hours = Monday 12:00 UTC.
        // Friday 16-17 consumes 1 hour, weekend is non-working, then
        // Monday 9-12 consumes the remaining 3. Mirrors the holiday
        // test below (1 hour Fri + 3 hours next working day).
        let cal = cal(serde_json::json!({
            "mon": [["09:00","17:00"]],
            "tue": [["09:00","17:00"]],
            "wed": [["09:00","17:00"]],
            "thu": [["09:00","17:00"]],
            "fri": [["09:00","17:00"]],
            "sat": [],
            "sun": []
        }));
        let start = Utc.with_ymd_and_hms(2026, 5, 1, 16, 0, 0).unwrap(); // Friday
        let end = add_business_minutes(start, 4 * 60, &cal, &HashSet::new());
        assert_eq!(end, Utc.with_ymd_and_hms(2026, 5, 4, 12, 0, 0).unwrap());
    }

    fn policy(id: i32, group: Option<i32>, is_default: bool) -> SlaPolicy {
        SlaPolicy {
            id,
            name: format!("p{id}"),
            target_response_minutes: Some(60),
            target_resolution_minutes: Some(240),
            working_calendar_id: Some(1),
            priority_filter: None,
            category_id_filter: None,
            assignee_group_id_filter: group,
            is_default,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: None,
            workspace_id: 1,
            no_sla: false,
            clock_start: "created".to_string(),
            // No need to backfill new ticket fields; the matcher
            // doesn't read them and Ticket is built per-test.
        }
    }

    #[test]
    fn a_policy_without_a_calendar_uses_its_workspaces_default() {
        let calendar = |id: i32, workspace_id: i32, is_default: bool| WorkingCalendar {
            id,
            workspace_id,
            is_default,
            ..cal(serde_json::json!({}))
        };
        let calendars: HashMap<i32, WorkingCalendar> = [
            calendar(1, 1, false),
            calendar(2, 1, true),
            calendar(3, 2, true),
        ]
        .into_iter()
        .map(|c| (c.id, c))
        .collect();
        let own = policy(1, None, false);
        assert_eq!(calendar_for_policy(&own, &calendars).map(|c| c.id), Some(1));
        let none = SlaPolicy {
            working_calendar_id: None,
            ..policy(2, None, false)
        };
        assert_eq!(
            calendar_for_policy(&none, &calendars).map(|c| c.id),
            Some(2),
            "its own workspace's default, not another workspace's"
        );
        let without_default: HashMap<i32, WorkingCalendar> =
            [calendar(5, 1, false), calendar(4, 1, false)]
                .into_iter()
                .map(|c| (c.id, c))
                .collect();
        assert_eq!(
            calendar_for_policy(&none, &without_default).map(|c| c.id),
            Some(4),
            "with no default, the workspace's first calendar"
        );
        assert!(calendar_for_policy(&none, &HashMap::new()).is_none());
    }

    fn ticket(assignee: Option<Uuid>) -> Ticket {
        Ticket {
            id: 1,
            uuid: uuid::Uuid::nil(),
            title: "t".into(),
            workflow_state_id: 2,
            priority: crate::models::TicketPriority::Medium,
            requester_uuid: None,
            assignee_uuid: assignee,
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
            created_by: None,
            closed_at: None,
            closed_by: None,
            category_id: None,
            submitted_via: None,
            guest_lookup_token: None,
            verification_state: None,
            origin_channel_id: None,
            triage_state: None,
            due_date: None,
            start_date: None,
            recurrence_rule: None,
            recurrence_template_id: None,
            resolution_notes: None,
            workspace_id: 1,
            first_response_at: None,
            sla_response_target_at: None,
            sla_response_breached_at: None,
            sla_resolution_target_at: None,
            sla_resolution_breached_at: None,
            spam_suspected: false,
            sla_clock_started_at: None,
            sla_paused_at: None,
            sla_override: "auto".to_string(),
            approval_state: None,
            number: 1,
            raised_by_staff: false,
        }
    }

    #[test]
    fn no_sla_policy_wins_precedence_but_yields_no_pill() {
        // A more-specific No-SLA policy beats the catch-all default (precedence
        // is unchanged), and a ticket it matches gets no pill.
        let mut no_sla_policy = policy(2, Some(10), false);
        no_sla_policy.no_sla = true;
        let default_policy = policy(1, None, true);
        let policies = vec![default_policy, no_sla_policy.clone()];
        let t = ticket(Some(Uuid::new_v4()));

        // pick_policy still selects the most-specific (group) policy.
        let picked = pick_policy(&policies, &t, &[10]).expect("a policy matches");
        assert_eq!(picked.id, 2);
        assert!(picked.no_sla);

        // But compute_pill on a No-SLA policy renders nothing, regardless of
        // configured targets / calendar.
        let calendar = cal(serde_json::json!({
            "mon": [["00:00", "23:59"]], "tue": [["00:00", "23:59"]],
            "wed": [["00:00", "23:59"]], "thu": [["00:00", "23:59"]],
            "fri": [["00:00", "23:59"]], "sat": [["00:00", "23:59"]],
            "sun": [["00:00", "23:59"]]
        }));
        let pill = compute_pill(
            &t,
            StateClock::Running,
            &no_sla_policy,
            &calendar,
            &HashSet::new(),
            Utc::now(),
        );
        assert!(pill.is_none(), "a No-SLA policy must produce no pill");
    }

    #[test]
    fn ticket_override_none_wins_over_matching_policy() {
        // The per-ticket escape hatch: override=none removes the SLA even when a
        // normal applies-policy matches.
        let mut t = ticket(None);
        t.sla_override = "none".to_string();
        let p = policy(1, None, true); // has targets + a calendar
        let pill = compute_pill(
            &t,
            StateClock::Running,
            &p,
            &all_hours_cal(),
            &HashSet::new(),
            Utc::now(),
        );
        assert!(pill.is_none(), "sla_override=none removes the SLA");
    }

    fn all_hours_cal() -> WorkingCalendar {
        cal(serde_json::json!({
            "mon": [["00:00", "23:59"]], "tue": [["00:00", "23:59"]],
            "wed": [["00:00", "23:59"]], "thu": [["00:00", "23:59"]],
            "fri": [["00:00", "23:59"]], "sat": [["00:00", "23:59"]],
            "sun": [["00:00", "23:59"]]
        }))
    }

    fn business_cal() -> WorkingCalendar {
        cal(serde_json::json!({
            "mon": [["09:00", "17:00"]], "tue": [["09:00", "17:00"]],
            "wed": [["09:00", "17:00"]], "thu": [["09:00", "17:00"]],
            "fri": [["09:00", "17:00"]], "sat": [], "sun": []
        }))
    }

    #[test]
    fn business_minutes_between_counts_within_day() {
        let start = Utc.with_ymd_and_hms(2026, 5, 4, 10, 0, 0).unwrap(); // Monday
        let end = start + Duration::hours(3);
        assert_eq!(
            business_minutes_between(start, end, &all_hours_cal(), &HashSet::new()),
            180
        );
    }

    #[test]
    fn business_minutes_between_skips_weekend() {
        // Friday 16:00 -> Monday 10:00 on a 09:00-17:00 M-F calendar:
        // Fri 16-17 = 60, weekend = 0, Mon 09-10 = 60 -> 120.
        let start = Utc.with_ymd_and_hms(2026, 5, 1, 16, 0, 0).unwrap(); // Friday
        let end = Utc.with_ymd_and_hms(2026, 5, 4, 10, 0, 0).unwrap(); // Monday
        assert_eq!(
            business_minutes_between(start, end, &business_cal(), &HashSet::new()),
            120
        );
    }

    fn activated_policy() -> SlaPolicy {
        let mut p = policy(1, None, true);
        p.clock_start = "activated".to_string();
        p.target_response_minutes = Some(60);
        p.target_resolution_minutes = None;
        p
    }

    #[test]
    fn activated_clock_not_started_yields_no_pill() {
        // Never activated (no anchor) and currently paused (backlog) -> no SLA.
        let t = ticket(None); // sla_clock_started_at = None
        let pill = compute_pill(
            &t,
            StateClock::Paused, // paused (in a pre-active state)
            &activated_policy(),
            &all_hours_cal(),
            &HashSet::new(),
            Utc::now(),
        );
        assert!(
            pill.is_none(),
            "an activated clock with no anchor + paused is not_started"
        );
    }

    #[test]
    fn activated_clock_falls_back_to_created_when_active_without_anchor() {
        // Pre-migration active ticket (no anchor, not paused) keeps an SLA,
        // anchored at created_at, and self-heals on its next transition.
        let mut t = ticket(None);
        t.created_at = Utc
            .with_ymd_and_hms(2026, 5, 4, 10, 0, 0)
            .unwrap()
            .naive_utc();
        let pill = compute_pill(
            &t,
            StateClock::Running,
            &activated_policy(),
            &all_hours_cal(),
            &HashSet::new(),
            Utc.with_ymd_and_hms(2026, 5, 4, 10, 30, 0).unwrap(),
        )
        .expect("active ticket keeps an SLA via the created fallback");
        // target = created + 60 min.
        assert_eq!(
            pill.response.unwrap().target_at,
            Utc.with_ymd_and_hms(2026, 5, 4, 11, 0, 0).unwrap()
        );
    }

    #[test]
    fn activated_clock_anchors_at_started_at_not_created() {
        // The fix: target is anchored to activation, not creation, so a ticket
        // that sat in backlog for days doesn't breach the instant it's activated.
        let mut t = ticket(None);
        t.created_at = Utc
            .with_ymd_and_hms(2026, 5, 1, 9, 0, 0)
            .unwrap()
            .naive_utc();
        t.sla_clock_started_at = Some(
            Utc.with_ymd_and_hms(2026, 5, 4, 10, 0, 0)
                .unwrap()
                .naive_utc(),
        ); // activated 3 days later
        let pill = compute_pill(
            &t,
            StateClock::Running,
            &activated_policy(),
            &all_hours_cal(),
            &HashSet::new(),
            Utc.with_ymd_and_hms(2026, 5, 4, 10, 5, 0).unwrap(),
        )
        .expect("started clock renders a pill");
        let resp = pill.response.unwrap();
        // target = activation + 60 min (NOT created + 60), and not breached.
        assert_eq!(
            resp.target_at,
            Utc.with_ymd_and_hms(2026, 5, 4, 11, 0, 0).unwrap()
        );
        assert!(
            !resp.breached,
            "freshly activated ticket is not instantly breached"
        );
    }

    #[test]
    fn created_clock_ignores_pause() {
        // A created-clock policy runs continuously from submission; the workflow
        // pause doesn't freeze it.
        let mut p = activated_policy();
        p.clock_start = "created".to_string();
        let mut t = ticket(None);
        t.created_at = Utc
            .with_ymd_and_hms(2026, 5, 4, 10, 0, 0)
            .unwrap()
            .naive_utc();
        let pill = compute_pill(
            &t,
            StateClock::Paused, // workflow says paused
            &p,
            &all_hours_cal(),
            &HashSet::new(),
            Utc.with_ymd_and_hms(2026, 5, 4, 10, 30, 0).unwrap(),
        )
        .expect("created clock renders a pill even while the state pauses");
        assert!(
            !pill.response.unwrap().paused,
            "created clock ignores the workflow pause"
        );
    }

    #[test]
    fn a_finished_ticket_stops_every_clock() {
        // Long past both targets, but finished: neither clock mode breaches.
        let mut t = ticket(None);
        t.created_at = Utc
            .with_ymd_and_hms(2026, 5, 4, 10, 0, 0)
            .unwrap()
            .naive_utc();
        t.sla_clock_started_at = Some(t.created_at);
        let later = Utc.with_ymd_and_hms(2026, 5, 9, 10, 0, 0).unwrap();
        for clock_start in ["created", "activated"] {
            let mut p = activated_policy();
            p.clock_start = clock_start.to_string();
            let pill = compute_pill(
                &t,
                StateClock::Stopped,
                &p,
                &all_hours_cal(),
                &HashSet::new(),
                later,
            )
            .expect("a pill");
            assert!(!pill.primary.breached, "{clock_start}");
            assert!(pill.primary.paused, "{clock_start}");
        }
    }

    /// A created-clock policy, response and resolution targets in minutes,
    /// and a ticket created at 10:00.
    fn two_timer_pill(
        response: i32,
        resolution: i32,
        first_response_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> SlaPill {
        let mut p = policy(1, None, true);
        p.target_response_minutes = Some(response);
        p.target_resolution_minutes = Some(resolution);
        let mut t = ticket(None);
        t.created_at = Utc
            .with_ymd_and_hms(2026, 5, 4, 10, 0, 0)
            .unwrap()
            .naive_utc();
        t.first_response_at = first_response_at.map(|at| at.naive_utc());
        compute_pill(
            &t,
            StateClock::Running,
            &p,
            &all_hours_cal(),
            &HashSet::new(),
            now,
        )
        .expect("a pill")
    }

    fn bucket(pill: &SlaPill) -> PolicyMatchCounts {
        let mut counts = PolicyMatchCounts::default();
        counts.add_pill(Some(pill));
        counts
    }

    /// The resolution target passed while the response is still due: the
    /// ticket has breached, so the pill and the count say so.
    #[test]
    fn a_breached_resolution_shows_while_the_response_is_due() {
        let pill = two_timer_pill(
            60,
            5,
            None,
            Utc.with_ymd_and_hms(2026, 5, 4, 10, 6, 0).unwrap(),
        );
        assert!(pill.primary.breached, "pill breached");
        assert_eq!(pill.primary.pill_color, "red");
        assert_eq!(bucket(&pill).breached, 1, "counted as breached");
    }

    /// A response that came after its target breached, and stays breached
    /// while the resolution is still on track.
    #[test]
    fn a_late_response_shows_breached_while_resolution_is_on_track() {
        let pill = two_timer_pill(
            60,
            240,
            Some(Utc.with_ymd_and_hms(2026, 5, 4, 11, 30, 0).unwrap()),
            Utc.with_ymd_and_hms(2026, 5, 4, 11, 40, 0).unwrap(),
        );
        assert!(pill.primary.breached, "pill breached");
        assert_eq!(pill.primary.pill_color, "red");
        assert_eq!(bucket(&pill).breached, 1, "counted as breached");
        // Red, but still counting down to the resolution target.
        assert_eq!(
            pill.primary.target_at,
            Utc.with_ymd_and_hms(2026, 5, 4, 14, 0, 0).unwrap()
        );
        assert_eq!(pill.primary.met_at, None);
        assert_eq!(pill.primary.seconds_remaining, Some(140 * 60));
    }

    /// A breach the breach job stamped stands while the ticket sits in a
    /// state that pauses the clock: the notification went out, so the pill
    /// and the count say breached, not paused.
    #[test]
    fn a_stamped_breach_shows_while_the_clock_is_paused() {
        let mut t = ticket(None);
        let created = Utc.with_ymd_and_hms(2026, 5, 4, 10, 0, 0).unwrap();
        t.created_at = created.naive_utc();
        t.sla_clock_started_at = Some(created.naive_utc());
        // Breached at 11:00, stamped a minute later, then moved to a pausing
        // state at 11:05.
        t.sla_response_breached_at = Some((created + Duration::minutes(61)).naive_utc());
        t.sla_paused_at = Some((created + Duration::minutes(65)).naive_utc());
        let pill = compute_pill(
            &t,
            StateClock::Paused,
            &activated_policy(),
            &all_hours_cal(),
            &HashSet::new(),
            created + Duration::minutes(90),
        )
        .expect("a pill");
        assert!(
            pill.response.as_ref().unwrap().breached,
            "response breached"
        );
        assert!(pill.primary.breached, "pill breached");
        assert_eq!(bucket(&pill).breached, 1, "counted as breached");
    }

    /// A stamp doesn't stand while its timer's target is still ahead: the
    /// target moved later since (a longer target, or time paused), and the
    /// recompute clears the stamp.
    #[test]
    fn a_stamp_before_the_target_does_not_stand() {
        let mut t = ticket(None);
        let created = Utc.with_ymd_and_hms(2026, 5, 4, 10, 0, 0).unwrap();
        t.created_at = created.naive_utc();
        t.sla_clock_started_at = Some((created + Duration::minutes(30)).naive_utc());
        t.sla_response_breached_at = Some((created + Duration::minutes(61)).naive_utc());
        let pill = compute_pill(
            &t,
            StateClock::Paused,
            &activated_policy(),
            &all_hours_cal(),
            &HashSet::new(),
            created + Duration::minutes(80),
        )
        .expect("a pill");
        assert!(!pill.primary.breached);
        assert!(pill.primary.paused);
    }

    /// With nothing breached, the timer due first leads, even when it's the
    /// resolution: the pill turns red as soon as anything breaches.
    #[test]
    fn the_unmet_timer_due_first_leads() {
        let pill = two_timer_pill(
            60,
            30,
            None,
            Utc.with_ymd_and_hms(2026, 5, 4, 10, 10, 0).unwrap(),
        );
        assert!(!pill.primary.breached);
        assert_eq!(
            pill.primary.target_at,
            Utc.with_ymd_and_hms(2026, 5, 4, 10, 30, 0).unwrap()
        );
        // A response met on time leaves the resolution leading.
        let pill = two_timer_pill(
            60,
            240,
            Some(Utc.with_ymd_and_hms(2026, 5, 4, 10, 20, 0).unwrap()),
            Utc.with_ymd_and_hms(2026, 5, 4, 10, 30, 0).unwrap(),
        );
        assert!(pill.primary.met_at.is_none());
        assert_eq!(
            pill.primary.target_at,
            Utc.with_ymd_and_hms(2026, 5, 4, 14, 0, 0).unwrap()
        );
    }

    #[test]
    fn pick_policy_group_filter_matches_when_assignee_in_group() {
        let group_policy = policy(2, Some(10), false);
        let default_policy = policy(1, None, true);
        let policies = vec![default_policy, group_policy];
        let t = ticket(Some(Uuid::new_v4()));
        // Assignee is in group 10, so the more-specific group policy wins.
        let picked = pick_policy(&policies, &t, &[10]).expect("a policy");
        assert_eq!(picked.id, 2);
    }

    #[test]
    fn pick_policy_group_filter_falls_back_to_default_when_assignee_not_in_group() {
        let policies = vec![policy(1, None, true), policy(2, Some(10), false)];
        let t = ticket(Some(Uuid::new_v4()));
        // Assignee is in groups 7 + 8 but not 10, so the group-scoped
        // policy is filtered out and we fall back to the default.
        let picked = pick_policy(&policies, &t, &[7, 8]).expect("a policy");
        assert_eq!(picked.id, 1);
    }

    #[test]
    fn pick_policy_group_filter_skips_when_ticket_unassigned() {
        let policies = vec![policy(2, Some(10), false)];
        let t = ticket(None);
        // No assignee, no group memberships, group-scoped policy
        // cannot match and there's no default to fall back to.
        assert!(pick_policy(&policies, &t, &[]).is_none());
    }

    // ---------------- Precedence coverage ----------------
    //
    // The matcher's job is to apply the precedence in this order:
    //   1. drop policies whose priority/category/group filters
    //      reject the ticket
    //   2. among survivors, non-default beats default (more specific)
    //   3. among same-default-status survivors, highest id wins
    //      (last-write semantics; explicit ordering would replace
    //      this when an admin UI ships)
    //
    // The tests below cover each branch of that decision tree.

    #[test]
    fn pick_policy_returns_none_when_no_policies() {
        assert!(pick_policy(&[], &ticket(None), &[]).is_none());
    }

    #[test]
    fn pick_policy_returns_none_when_no_policy_matches_and_no_default() {
        // Only a group-scoped policy exists and the assignee isn't
        // in that group — no fallback.
        let policies = vec![policy(1, Some(99), false)];
        let t = ticket(Some(Uuid::new_v4()));
        assert!(pick_policy(&policies, &t, &[7]).is_none());
    }

    #[test]
    fn pick_policy_picks_unfiltered_default_as_catch_all() {
        let policies = vec![policy(1, None, true)];
        let picked = pick_policy(&policies, &ticket(None), &[]).expect("a policy");
        assert_eq!(picked.id, 1);
    }

    #[test]
    fn pick_policy_higher_id_wins_among_non_defaults() {
        // Three unfiltered non-defaults — all match every ticket;
        // the matcher's tiebreak is highest id.
        let policies = vec![
            policy(1, None, false),
            policy(5, None, false),
            policy(3, None, false),
        ];
        let picked = pick_policy(&policies, &ticket(None), &[]).expect("a policy");
        assert_eq!(picked.id, 5);
    }

    #[test]
    fn pick_policy_non_default_beats_default_even_with_lower_id() {
        // Specificity beats id: even though the default has the
        // higher id, the non-default is more specific so it wins.
        let policies = vec![policy(99, None, true), policy(1, None, false)];
        let picked = pick_policy(&policies, &ticket(None), &[]).expect("a policy");
        assert_eq!(picked.id, 1);
    }

    #[test]
    fn pick_policy_two_defaults_picks_highest_id() {
        // The DB doesn't enforce uniqueness on is_default; if two
        // defaults end up flagged the matcher still picks
        // deterministically.
        let policies = vec![
            policy(1, None, true),
            policy(7, None, true),
            policy(3, None, true),
        ];
        let picked = pick_policy(&policies, &ticket(None), &[]).expect("a policy");
        assert_eq!(picked.id, 7);
    }

    #[test]
    fn pick_policy_priority_filter_rejects_mismatched_ticket() {
        let mut high_only = policy(2, None, false);
        high_only.priority_filter = Some("high".into());
        let policies = vec![policy(1, None, true), high_only];
        // Ticket default priority is Medium; the high-only policy
        // is filtered out and the default takes over.
        let picked = pick_policy(&policies, &ticket(None), &[]).expect("a policy");
        assert_eq!(picked.id, 1);
    }

    #[test]
    fn pick_policy_priority_filter_accepts_matching_ticket() {
        let mut high_only = policy(2, None, false);
        high_only.priority_filter = Some("high".into());
        let policies = vec![policy(1, None, true), high_only];
        let mut t = ticket(None);
        t.priority = crate::models::TicketPriority::High;
        let picked = pick_policy(&policies, &t, &[]).expect("a policy");
        assert_eq!(picked.id, 2);
    }

    /// A filter stored before saving checked it is read by the one parser: a
    /// legacy name matches the priority it means, and one naming no priority
    /// matches no ticket.
    #[test]
    fn pick_policy_reads_a_stored_priority_filter_leniently() {
        let mut legacy = policy(2, None, false);
        legacy.priority_filter = Some("normal".into());
        let mut stray = policy(3, None, false);
        stray.priority_filter = Some("critical".into());
        let policies = vec![policy(1, None, true), legacy, stray];
        let mut t = ticket(None);
        t.priority = crate::models::TicketPriority::Medium;
        assert_eq!(pick_policy(&policies, &t, &[]).expect("a policy").id, 2);
        t.priority = crate::models::TicketPriority::Urgent;
        assert_eq!(pick_policy(&policies, &t, &[]).expect("a policy").id, 1);
    }

    #[test]
    fn pick_policy_category_filter_rejects_when_ticket_has_no_category() {
        let mut cat_only = policy(2, None, false);
        cat_only.category_id_filter = Some(42);
        let policies = vec![policy(1, None, true), cat_only];
        // ticket(...) builds with category_id = None.
        let picked = pick_policy(&policies, &ticket(None), &[]).expect("a policy");
        assert_eq!(picked.id, 1);
    }

    #[test]
    fn pick_policy_combined_filters_all_must_match() {
        // Policy requires priority = high AND category = 42; ticket
        // satisfies one at a time, then both.
        let mut combo = policy(2, None, false);
        combo.priority_filter = Some("high".into());
        combo.category_id_filter = Some(42);
        let policies = vec![policy(1, None, true), combo];
        let mut t = ticket(None);
        t.priority = crate::models::TicketPriority::High;
        t.category_id = Some(7);
        // Priority matches, category doesn't -> default wins.
        assert_eq!(pick_policy(&policies, &t, &[]).unwrap().id, 1);
        // Flip category to match -> combo wins.
        t.category_id = Some(42);
        assert_eq!(pick_policy(&policies, &t, &[]).unwrap().id, 2);
    }

    #[test]
    fn pick_policy_result_independent_of_input_order() {
        // Same set of policies in two orderings must yield the same
        // pick — the matcher's tiebreak rules are total, not
        // input-ordering-dependent.
        let a = policy(1, None, true);
        let b = policy(2, None, false);
        let t = ticket(None);
        let order_ab = vec![a.clone(), b.clone()];
        let order_ba = vec![b, a];
        assert_eq!(
            pick_policy(&order_ab, &t, &[]).unwrap().id,
            pick_policy(&order_ba, &t, &[]).unwrap().id,
        );
    }

    #[test]
    fn add_business_minutes_skips_holiday() {
        let cal = cal(serde_json::json!({
            "mon": [["09:00","17:00"]],
            "tue": [["09:00","17:00"]],
            "wed": [["09:00","17:00"]],
            "thu": [["09:00","17:00"]],
            "fri": [["09:00","17:00"]],
            "sat": [],
            "sun": []
        }));
        let mut holidays = HashSet::new();
        holidays.insert(NaiveDate::from_ymd_opt(2026, 5, 4).unwrap()); // Monday holiday
        let start = Utc.with_ymd_and_hms(2026, 5, 1, 16, 0, 0).unwrap(); // Friday
        let end = add_business_minutes(start, 4 * 60, &cal, &holidays);
        // Friday 16-17 + Tuesday 9-12 = 4 hours
        assert_eq!(end, Utc.with_ymd_and_hms(2026, 5, 5, 12, 0, 0).unwrap());
    }

    /// A created-clock policy over an always-open calendar (1h response, 2h
    /// resolution). Highest id and no filters, so it wins every ticket in the
    /// test transaction.
    fn created_clock_policy(conn: &mut crate::db::DbConnection) -> SlaPolicy {
        use crate::repository::sla_admin::{
            create_calendar, create_policy, SlaPolicyBody, WorkingCalendarBody,
        };
        let day = serde_json::json!([["00:00", "23:59"]]);
        let calendar = create_calendar(
            conn,
            WorkingCalendarBody {
                name: "Always open".into(),
                timezone: None,
                schedule: serde_json::json!({
                    "mon": day, "tue": day, "wed": day, "thu": day,
                    "fri": day, "sat": day, "sun": day,
                }),
                is_default: Some(false),
            },
            None,
        )
        .unwrap();
        create_policy(
            conn,
            SlaPolicyBody {
                name: "Created clock".into(),
                target_response_minutes: Some(60),
                target_resolution_minutes: Some(120),
                working_calendar_id: Some(calendar.id),
                priority_filter: None,
                category_id_filter: None,
                assignee_group_id_filter: None,
                is_default: Some(false),
                no_sla: Some(false),
                clock_start: Some("created".into()),
            },
            None,
        )
        .unwrap()
    }

    fn ticket_in(
        conn: &mut crate::db::DbConnection,
        title: &str,
        category: crate::models::WorkflowStateCategory,
    ) -> Ticket {
        use crate::schema::tickets;
        use diesel::prelude::*;
        let user = crate::test_helpers::TestFixtures::create_user(conn, title, "user");
        let ticket =
            crate::test_helpers::TestFixtures::create_ticket(conn, title, Some(user.uuid), None);
        let state = crate::repository::workflow_states::first_in_category(conn, category).unwrap();
        diesel::update(tickets::table.find(ticket.id))
            .set(tickets::workflow_state_id.eq(state.id))
            .get_result(conn)
            .unwrap()
    }

    #[test]
    fn closing_a_ticket_stops_a_created_clock() {
        use crate::models::WorkflowStateCategory;
        use crate::schema::tickets;
        use diesel::prelude::*;
        let mut conn = crate::test_helpers::setup_test_connection();
        created_clock_policy(&mut conn);
        let open = ticket_in(&mut conn, "sla_close", WorkflowStateCategory::Active);
        // Opened three hours ago: both targets have passed.
        diesel::update(tickets::table.find(open.id))
            .set(tickets::created_at.eq(Utc::now().naive_utc() - Duration::hours(3)))
            .execute(&mut conn)
            .unwrap();
        let done = crate::repository::workflow_states::first_in_category(
            &mut conn,
            WorkflowStateCategory::Done,
        )
        .unwrap();

        let closed = crate::repository::tickets::update_ticket_partial(
            &mut conn,
            open.id,
            crate::models::TicketUpdate {
                workflow_state_id: Some(done.id),
                ..Default::default()
            },
            None,
        )
        .unwrap();

        let targets: (Option<NaiveDateTime>, Option<NaiveDateTime>) = tickets::table
            .find(closed.id)
            .select((
                tickets::sla_response_target_at,
                tickets::sla_resolution_target_at,
            ))
            .first(&mut conn)
            .unwrap();
        assert_eq!(targets, (None, None), "nothing left for the breach scan");
        let pill = pill_json_for_ticket(&mut conn, &closed);
        assert_eq!(pill["breached"], false, "{pill}");
    }

    #[test]
    fn merged_tickets_are_not_counted_as_open() {
        use crate::models::WorkflowStateCategory;
        let mut conn = crate::test_helpers::setup_test_connection();
        let policy = created_clock_policy(&mut conn);
        let counted = |conn: &mut crate::db::DbConnection| {
            scan_open_ticket_buckets(conn, i64::MAX)
                .unwrap()
                .by_policy
                .get(&policy.id)
                .map_or(0, |c| c.total)
        };
        let before = counted(&mut conn);
        ticket_in(&mut conn, "sla_merged", WorkflowStateCategory::Merged);
        assert_eq!(counted(&mut conn), before);
    }
}
