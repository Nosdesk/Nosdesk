//! SLA policy + working calendar reads.
//!
//! Bootstrap calls `load_for_pill_computation` once and feeds the
//! results into `services::sla::compute_pill` for every ticket.
//! Two hot-path ergonomics decisions:
//!
//! - `holidays_by_calendar` is a `HashMap<i32, HashSet<NaiveDate>>`
//!   so the per-ticket loop hits an O(1) lookup rather than a
//!   linear scan through Vec<WorkingCalendarHoliday>.
//! - `calendars_by_id` is a `HashMap<i32, WorkingCalendar>` so the
//!   policy → calendar resolve is one map lookup per ticket.

use chrono::{Datelike, NaiveDate, Utc};
use diesel::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::db::DbConnection;
use crate::models::{SlaPolicy, WorkingCalendar, WorkingCalendarHoliday};

pub struct SlaContext {
    pub policies: Vec<SlaPolicy>,
    pub calendars_by_id: HashMap<i32, WorkingCalendar>,
    pub holidays_by_calendar: HashMap<i32, HashSet<chrono::NaiveDate>>,
}

pub fn load_for_pill_computation(conn: &mut DbConnection) -> QueryResult<SlaContext> {
    use crate::schema::{sla_policies, working_calendar_holidays, working_calendars};

    let policies: Vec<SlaPolicy> = sla_policies::table.load(conn)?;
    let calendar_rows: Vec<WorkingCalendar> = working_calendars::table.load(conn)?;
    let holiday_rows: Vec<WorkingCalendarHoliday> = working_calendar_holidays::table.load(conn)?;

    let mut calendars_by_id: HashMap<i32, WorkingCalendar> = HashMap::new();
    for cal in calendar_rows {
        calendars_by_id.insert(cal.id, cal);
    }
    let current_year = Utc::now().year();
    let mut holidays_by_calendar: HashMap<i32, HashSet<chrono::NaiveDate>> = HashMap::new();
    for h in holiday_rows {
        let bucket = holidays_by_calendar.entry(h.calendar_id).or_default();
        for d in expand_holiday(&h, current_year) {
            bucket.insert(d);
        }
    }

    Ok(SlaContext {
        policies,
        calendars_by_id,
        holidays_by_calendar,
    })
}

/// Expand one holiday row into the set of concrete dates the engine
/// needs in its HashSet for `current_year`'s arithmetic. Single-date
/// rows pass through; annual rows generate MM-DD for the year window
/// the engine could touch (created tickets up to ~year-old, target
/// projections months out).
///
/// Window is `current_year - 1` through `current_year + 1` — three
/// years per row, cheap and covers all realistic SLA spans.
pub fn expand_holiday(h: &WorkingCalendarHoliday, current_year: i32) -> Vec<NaiveDate> {
    if h.recurrence != "annual" {
        return vec![h.date];
    }
    let (m, d) = (h.date.month(), h.date.day());
    let mut out = Vec::with_capacity(3);
    for year in (current_year - 1)..=(current_year + 1) {
        if let Some(date) = NaiveDate::from_ymd_opt(year, m, d) {
            out.push(date);
        }
        // Skips Feb 29 in non-leap years, which is the desired
        // behaviour (the day genuinely doesn't exist that year; the
        // admin can add an explicit Mar 1 override if they want).
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holiday(year: i32, month: u32, day: u32, recurrence: &str) -> WorkingCalendarHoliday {
        WorkingCalendarHoliday {
            id: 1,
            calendar_id: 1,
            date: NaiveDate::from_ymd_opt(year, month, day).unwrap(),
            label: None,
            workspace_id: 1,
            recurrence: recurrence.into(),
        }
    }

    #[test]
    fn expand_none_returns_only_the_stored_date() {
        let dates = expand_holiday(&holiday(2026, 12, 25, "none"), 2030);
        assert_eq!(dates, vec![NaiveDate::from_ymd_opt(2026, 12, 25).unwrap()]);
    }

    #[test]
    fn expand_annual_returns_three_years_centered_on_current() {
        let dates = expand_holiday(&holiday(2026, 12, 25, "annual"), 2026);
        assert_eq!(
            dates,
            vec![
                NaiveDate::from_ymd_opt(2025, 12, 25).unwrap(),
                NaiveDate::from_ymd_opt(2026, 12, 25).unwrap(),
                NaiveDate::from_ymd_opt(2027, 12, 25).unwrap(),
            ]
        );
    }

    #[test]
    fn expand_annual_skips_feb_29_in_non_leap_years() {
        // Seed date is 2024-02-29 (leap year). Window is 2023-2025;
        // only 2024 is a leap year, so only one concrete date
        // materialises.
        let dates = expand_holiday(&holiday(2024, 2, 29, "annual"), 2024);
        assert_eq!(dates, vec![NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()]);
    }
}
