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
//! 2. **Pill computation** — `compute_pill(ticket, paused, policy,
//!    calendar, holidays, now)` returns the spec'd CardData.sla
//!    payload `{ target_at, breached, paused, pill_color,
//!    seconds_remaining }`. Whether a ticket pauses the clock is the
//!    caller's responsibility, derived from the workflow state's
//!    own `pauses_sla` flag (admin-editable, defaults from the
//!    category at create time).
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
/// `paused` is the workflow state's own `pauses_sla` flag (resolved
/// at the caller from `WorkflowState::pauses_sla`); when true the
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
    paused: bool,
    policy: &SlaPolicy,
    calendar: &WorkingCalendar,
    holidays: &HashSet<NaiveDate>,
    now: DateTime<Utc>,
) -> Option<SlaPill> {
    // A No-SLA policy that wins matching means this ticket has no SLA: no pill.
    // Both the bootstrap read and the recompute path route through here, so this
    // one check covers every surface (and clears the materialised targets, since
    // callers treat `None` as "no SLA").
    if policy.no_sla {
        return None;
    }

    let created_utc = DateTime::<Utc>::from_naive_utc_and_offset(ticket.created_at, Utc);

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
    let (response_target, resolution_target) =
        pill.as_ref().map(targets_from_pill).unwrap_or((None, None));
    set_sla_targets(conn, ticket.id, response_target, resolution_target);
    pill.and_then(|p| serde_json::to_value(p).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// Load every input the engine needs for one ticket and run
/// `compute_pill`. Returns `None` when any link in the chain is
/// missing (no matching policy, policy without a calendar, calendar
/// row gone, no configured targets) — callers either return null JSON
/// or clear the materialised columns accordingly.
fn load_pill_for_ticket(conn: &mut crate::db::DbConnection, ticket: &Ticket) -> Option<SlaPill> {
    use crate::schema::{
        sla_policies, workflow_states, working_calendar_holidays, working_calendars,
    };
    use diesel::prelude::*;

    let policies: Vec<SlaPolicy> = sla_policies::table.load(conn).ok()?;
    let group_ids = ticket
        .assignee_uuid
        .and_then(|u| crate::repository::groups::get_group_ids_for_user(conn, &u).ok())
        .unwrap_or_default();
    let policy = pick_policy(&policies, ticket, &group_ids)?;
    let cal_id = policy.working_calendar_id?;
    let calendar: WorkingCalendar = working_calendars::table.find(cal_id).first(conn).ok()?;
    // Pull the full rows so annual-recurrence holidays expand into
    // their concrete dates for the year window the engine touches.
    // expand_holiday lives in the repository so the bootstrap path
    // and this per-ticket path share the same rule.
    let holiday_rows: Vec<WorkingCalendarHoliday> = working_calendar_holidays::table
        .filter(working_calendar_holidays::calendar_id.eq(cal_id))
        .load(conn)
        .unwrap_or_default();
    let current_year = Utc::now().year();
    let holidays: HashSet<NaiveDate> = holiday_rows
        .iter()
        .flat_map(|h| crate::repository::sla::expand_holiday(h, current_year))
        .collect();
    // Default to paused so a missing state row (shouldn't happen but
    // can if a state was hard-deleted) doesn't accidentally start
    // counting time against an unresolvable category.
    let paused = workflow_states::table
        .find(ticket.workflow_state_id)
        .select(workflow_states::pauses_sla)
        .first::<bool>(conn)
        .unwrap_or(true);

    compute_pill(ticket, paused, policy, &calendar, &holidays, Utc::now())
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
    let priority_str = match ticket.priority {
        crate::models::TicketPriority::None => "none",
        crate::models::TicketPriority::Low => "low",
        crate::models::TicketPriority::Medium => "medium",
        crate::models::TicketPriority::High => "high",
        crate::models::TicketPriority::Urgent => "urgent",
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
    /// Breached wins over paused so a paused ticket whose response
    /// timer was met late still counts as breached — the more
    /// actionable signal — and the count matches the live pill.
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

    // Open = not in a terminal category. Two cheap queries: pick the
    // open state ids + their pauses_sla flag, then load tickets in
    // those states. Ticket doesn't derive Selectable so we avoid the
    // inner-join select tuple.
    let open_states: Vec<(i32, bool)> = workflow_states::table
        .filter(workflow_states::category.ne(WorkflowStateCategory::Done))
        .filter(workflow_states::category.ne(WorkflowStateCategory::Cancelled))
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
        let paused = pause_by_state
            .get(&ticket.workflow_state_id)
            .copied()
            .unwrap_or(true);
        let assignee_groups = ticket
            .assignee_uuid
            .and_then(|u| groups_by_assignee.get(&u))
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let Some(policy) = pick_policy(&ctx.policies, &ticket, assignee_groups) else {
            continue;
        };
        // A No-SLA policy means this ticket has no SLA — exclude it from the SLA
        // health counts rather than defaulting it into on_track / total.
        if policy.no_sla {
            continue;
        }

        // No calendar attached -> no pill; the policy still matches
        // the ticket so it counts toward `total` but lands in
        // `on_track` as a neutral default.
        let pill = policy
            .working_calendar_id
            .and_then(|cal_id| {
                ctx.calendars_by_id.get(&cal_id).map(|calendar| {
                    let holidays = ctx
                        .holidays_by_calendar
                        .get(&cal_id)
                        .cloned()
                        .unwrap_or_default();
                    compute_pill(&ticket, paused, policy, calendar, &holidays, now)
                })
            })
            .flatten();

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
            // No need to backfill new ticket fields; the matcher
            // doesn't read them and Ticket is built per-test.
        }
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
            false,
            &no_sla_policy,
            &calendar,
            &HashSet::new(),
            Utc::now(),
        );
        assert!(pill.is_none(), "a No-SLA policy must produce no pill");
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
}
