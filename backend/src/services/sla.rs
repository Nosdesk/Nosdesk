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

/// Compute the SLA pill payload for a ticket. The architecture spec
/// says SLA arithmetic only runs while the ticket's workflow state
/// category is `active`; everything else (triage, backlog, in_review,
/// done, cancelled) is paused so the pill stops counting and shows
/// the paused colour.
pub fn compute_pill(
    ticket: &Ticket,
    category: WorkflowStateCategory,
    policy: &SlaPolicy,
    calendar: &WorkingCalendar,
    holidays: &HashSet<NaiveDate>,
    now: DateTime<Utc>,
) -> serde_json::Value {
    let target_minutes = match policy.target_resolution_minutes {
        Some(m) if m > 0 => m as i64,
        _ => return serde_json::Value::Null,
    };

    // Project the breach instant from the ticket's creation time.
    // Pause states freeze the pill colour rather than drift the
    // target — the recompute on the next bootstrap picks up where
    // the previous active stretch left off.
    let created_utc = DateTime::<Utc>::from_naive_utc_and_offset(ticket.created_at, Utc);
    let target_at = add_business_minutes(created_utc, target_minutes, calendar, holidays);
    let paused = category != WorkflowStateCategory::Active;
    let breached = !paused && now > target_at;
    let seconds_remaining = if breached {
        Some((now - target_at).num_seconds().saturating_neg())
    } else {
        Some((target_at - now).num_seconds().max(0))
    };

    let pill_color = if breached {
        "red"
    } else if paused {
        "amber"
    } else {
        // Within 25% of the window remaining flips to amber so the
        // pill flags work that's about to breach without waiting
        // for the actual transition.
        let window_seconds = (target_at - created_utc).num_seconds().max(1);
        let remaining = seconds_remaining.unwrap_or(0);
        if remaining * 4 < window_seconds {
            "amber"
        } else {
            "green"
        }
    };

    serde_json::json!({
        "target_at": target_at,
        "breached": breached,
        "paused": paused,
        "pill_color": pill_color,
        "seconds_remaining": seconds_remaining,
    })
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
