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
//! 2. **Pill computation** — `compute_pill(ticket, policy, calendar,
//!    holidays, now)` returns the spec'd CardData.sla payload:
//!    `{ target_at, breached, paused, pill_color, seconds_remaining }`.
//!    Tickets in non-active workflow categories are paused
//!    (architecture doc § 6: "a ticket is in progress for SLA
//!    purposes whenever its state's category is `active`").
//!
//! This module is read-only. SLA pills are derived on every read;
//! there's no separate `sla_application` row to maintain. If the
//! perf tax becomes real (~thousands of tickets per bootstrap),
//! the natural next step is to materialise pill values onto the
//! ticket row and invalidate via sync_actions on workflow_state
//! transitions.

use chrono::{
    DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Timelike, Utc, Weekday,
};
use chrono_tz::Tz;
use std::collections::HashSet;

use crate::models::{
    SlaPolicy, Ticket, WorkflowStateCategory, WorkingCalendar, WorkingCalendarHoliday,
};

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

/// Full SLA payload for one ticket. The most-urgent active timer's
/// fields are flattened to the top level so every v1 consumer
/// (`TicketRow` pill column, the filter facet, `KanbanBoard`) keeps
/// reading `sla.breached` / `sla.paused` / `sla.target_at` / etc.
/// unchanged — they now reflect whichever timer is currently most
/// at risk. The nested `response` + `resolution` sub-objects are new,
/// additive, and consumed by the preview pane to stack both timers.
///
/// "Most urgent" = the response timer when it's still active
/// (`first_response_at IS NULL`); the resolution timer otherwise.
/// Mirrors the architecture spec: pre-first-response, missing the
/// response target is the louder signal; after it's met, the
/// resolution timer is the only thing still counting.
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
/// optional met-at moment. Shared by response + resolution: the
/// response timer passes `met_at = Some(first_response_at)` once a
/// staff comment lands; the resolution timer leaves it `None`.
fn compute_timer(
    target_minutes: i64,
    start_from: DateTime<Utc>,
    met_at: Option<DateTime<Utc>>,
    paused: bool,
    calendar: &WorkingCalendar,
    holidays: &HashSet<NaiveDate>,
    now: DateTime<Utc>,
) -> SlaTimer {
    let target_at = add_business_minutes(start_from, target_minutes, calendar, holidays);

    // A met timer is judged against when it was met, not the wall
    // clock — so a response that lands 1m before the target is "met
    // on time" even if we're observing 3h later.
    let breached = match met_at {
        Some(met) => met > target_at,
        None if paused => false,
        None => now > target_at,
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

/// Compute the SLA pill payload for a ticket — both response +
/// resolution timers, gated on which policy targets are configured.
/// The architecture spec says SLA arithmetic only runs while the
/// ticket's workflow state category is `active`; everything else
/// (triage, backlog, in_review, done, cancelled) is paused so the
/// timers stop counting. The response timer also stops counting once
/// `first_response_at` is stamped, regardless of pause state — at
/// that point the response was either met or breached, and the wall
/// clock has nothing left to say about it.
///
/// Returns `None` when the policy has neither a response target nor a
/// resolution target (no pill to render). Otherwise returns at least
/// one timer; the other is `None` when its target isn't configured.
pub fn compute_pill(
    ticket: &Ticket,
    category: WorkflowStateCategory,
    policy: &SlaPolicy,
    calendar: &WorkingCalendar,
    holidays: &HashSet<NaiveDate>,
    now: DateTime<Utc>,
) -> Option<SlaPill> {
    let created_utc = DateTime::<Utc>::from_naive_utc_and_offset(ticket.created_at, Utc);
    let paused = category != WorkflowStateCategory::Active;

    let response = policy
        .target_response_minutes
        .filter(|m| *m > 0)
        .map(|minutes| {
            let met_at = ticket
                .first_response_at
                .map(|t| DateTime::<Utc>::from_naive_utc_and_offset(t, Utc));
            compute_timer(
                minutes as i64,
                created_utc,
                met_at,
                paused,
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
                created_utc,
                None,
                paused,
                calendar,
                holidays,
                now,
            )
        });

    // Pick the most-urgent active timer to flatten as `primary`. The
    // response timer wins while it's still ticking (first_response_at
    // not yet stamped); once met, the resolution timer takes over.
    // Fallback to whichever exists so a policy with only one target
    // configured still renders a pill.
    let primary = match (&response, &resolution) {
        (Some(r), _) if r.met_at.is_none() => r.clone(),
        (_, Some(res)) => res.clone(),
        (Some(r), None) => r.clone(),
        (None, None) => return None,
    };

    Some(SlaPill {
        primary,
        response,
        resolution,
    })
}

/// Load the SLA context for one ticket and return its pill payload.
///
/// The bootstrap path loads policies / calendars / holidays once for
/// the whole workspace and reuses them across every ticket. Mutation
/// handlers (status change, priority change, etc.) only need to
/// recompute one ticket at a time, so this helper does a per-ticket
/// load instead of dragging the full context through every caller.
///
/// Used by `repository::tickets::update_ticket_partial` so the
/// `sync_action` it broadcasts on a pill-affecting field change carries
/// an up-to-date pill — without this, open clients keep showing the
/// previous (now stale) pill until the next bootstrap. Returns
/// `Value::Null` when no policy matches; the frontend treats that as
/// "no SLA on this ticket" and hides the pill.
/// Recompute one ticket's SLA pill and persist the materialised
/// target timestamps in the same call. Used by mutation paths
/// (status / priority / category change, first-response stamp, the
/// breach-detection sweep) so the `sla_response_target_at` /
/// `sla_resolution_target_at` columns the breach job scans against
/// stay in lockstep with the JSON pill the frontend renders. Returns
/// the pill JSON to slot into the `ticket.sla_updated` sync_action;
/// `Value::Null` when no policy applies (and the materialised columns
/// are cleared so the breach scan ignores the row).
pub fn recompute_and_stamp_sla_for_ticket(
    conn: &mut crate::db::DbConnection,
    ticket: &Ticket,
) -> serde_json::Value {
    let pill = load_pill_for_ticket(conn, ticket);
    let (response_target, resolution_target) = pill
        .as_ref()
        .map(targets_from_pill)
        .unwrap_or((None, None));
    set_sla_targets(conn, ticket.id, response_target, resolution_target);
    pill.and_then(|p| serde_json::to_value(p).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// Load every input the engine needs for one ticket and run
/// `compute_pill`. Returns `None` when any link in the chain is
/// missing (no matching policy, policy without a calendar, calendar
/// row gone, no configured targets) — callers either return null JSON
/// or clear the materialised columns accordingly.
fn load_pill_for_ticket(
    conn: &mut crate::db::DbConnection,
    ticket: &Ticket,
) -> Option<SlaPill> {
    use crate::schema::{
        sla_policies, working_calendar_holidays, working_calendars, workflow_states,
    };
    use diesel::prelude::*;

    let policies: Vec<SlaPolicy> = sla_policies::table.load(conn).ok()?;
    let policy = pick_policy(&policies, ticket)?;
    let cal_id = policy.working_calendar_id?;
    let calendar: WorkingCalendar = working_calendars::table.find(cal_id).first(conn).ok()?;
    let holidays: HashSet<NaiveDate> = working_calendar_holidays::table
        .filter(working_calendar_holidays::calendar_id.eq(cal_id))
        .select(working_calendar_holidays::date)
        .load::<NaiveDate>(conn)
        .map(|v| v.into_iter().collect())
        .unwrap_or_default();
    let category = workflow_states::table
        .find(ticket.workflow_state_id)
        .select(workflow_states::category)
        .first::<WorkflowStateCategory>(conn)
        .unwrap_or(WorkflowStateCategory::Backlog);

    compute_pill(ticket, category, policy, &calendar, &holidays, Utc::now())
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

/// Persist the materialised target columns on a ticket. `None` clears
/// the column so the partial scan index ignores the row. Failures are
/// logged rather than propagated — a missed stamp is self-healing on
/// the next mutation or the next breach-detection sweep, and we don't
/// want one stamp failure to roll back the caller's transaction.
fn set_sla_targets(
    conn: &mut crate::db::DbConnection,
    ticket_id: i32,
    response_target: Option<chrono::NaiveDateTime>,
    resolution_target: Option<chrono::NaiveDateTime>,
) {
    use crate::schema::tickets;
    use diesel::prelude::*;

    if let Err(e) = diesel::update(tickets::table.find(ticket_id))
        .set((
            tickets::sla_response_target_at.eq(response_target),
            tickets::sla_resolution_target_at.eq(resolution_target),
        ))
        .execute(conn)
    {
        tracing::warn!(ticket_id, error = %e, "set_sla_targets failed");
    }
}

/// Pick the most-specific policy that matches a ticket. Highest-id
/// match wins, with the workspace default as a fallback.
pub fn pick_policy<'a>(policies: &'a [SlaPolicy], ticket: &Ticket) -> Option<&'a SlaPolicy> {
    let priority_str = match ticket.priority {
        crate::models::TicketPriority::Low => "low",
        crate::models::TicketPriority::Medium => "medium",
        crate::models::TicketPriority::High => "high",
    };
    let mut best: Option<&SlaPolicy> = None;
    for policy in policies {
        let priority_ok = policy
            .priority_filter
            .as_deref()
            .map(|p| p == priority_str)
            .unwrap_or(true);
        let category_ok = policy
            .category_id_filter
            .map(|c| Some(c) == ticket.category_id)
            .unwrap_or(true);
        if !priority_ok || !category_ok {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

// Suppress an unused-import warning on `WorkingCalendarHoliday`
// while no consumer uses the struct directly (the SLA service
// works with the parsed `HashSet<NaiveDate>` hot-path shape).
#[allow(dead_code)]
fn _holiday_marker(_h: &WorkingCalendarHoliday) {}
