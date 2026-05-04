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
    let mut holidays_by_calendar: HashMap<i32, HashSet<chrono::NaiveDate>> = HashMap::new();
    for h in holiday_rows {
        holidays_by_calendar
            .entry(h.calendar_id)
            .or_default()
            .insert(h.date);
    }

    Ok(SlaContext { policies, calendars_by_id, holidays_by_calendar })
}
