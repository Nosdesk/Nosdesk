//! Turning a ticket's dates into moments.
//!
//! `tickets.due_date` is a floating calendar day, stored as midnight with no
//! zone: "21 November" means the 21st for every viewer. When the server
//! needs a real moment (the `until_due_at` rule clock), the day resolves in
//! the workspace's timezone (`site_settings.default_timezone`, else UTC),
//! the same moment for everyone. This module is the one place that rule
//! lives, so a later due time can extend it.

use chrono::{DateTime, Days, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// When a ticket due on `due_date` is due: the end of that day, that is the
/// first moment of the next day, in the workspace timezone `tz`. Only the
/// calendar day of the stored value is read.
pub fn due_instant(due_date: DateTime<Utc>, tz: Tz) -> DateTime<Utc> {
    let next_day = due_date
        .date_naive()
        .checked_add_days(Days::new(1))
        .unwrap_or(chrono::NaiveDate::MAX);
    let midnight = next_day.and_time(NaiveTime::MIN);
    // A zone that skips midnight (a daylight saving change at 00:00) starts
    // the day at its first moment after the gap.
    tz.from_local_datetime(&midnight)
        .earliest()
        .or_else(|| {
            tz.from_local_datetime(&(midnight + chrono::Duration::hours(1)))
                .earliest()
        })
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|| midnight.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
    }

    #[test]
    fn a_due_day_ends_at_the_workspace_midnight() {
        // Sydney in November is UTC+11: the 21st ends at 13:00 UTC on the 21st.
        assert_eq!(
            due_instant(day(2026, 11, 21), chrono_tz::Australia::Sydney),
            Utc.with_ymd_and_hms(2026, 11, 21, 13, 0, 0).unwrap()
        );
        // New York in November is UTC-5: the 21st ends at 05:00 UTC on the 22nd.
        assert_eq!(
            due_instant(day(2026, 11, 21), chrono_tz::America::New_York),
            Utc.with_ymd_and_hms(2026, 11, 22, 5, 0, 0).unwrap()
        );
        assert_eq!(due_instant(day(2026, 11, 21), Tz::UTC), day(2026, 11, 22));
    }

    #[test]
    fn only_the_calendar_day_is_read() {
        let stray = Utc.with_ymd_and_hms(2026, 11, 21, 17, 42, 0).unwrap();
        assert_eq!(due_instant(stray, Tz::UTC), day(2026, 11, 22));
    }

    #[test]
    fn a_zone_that_skips_midnight_starts_the_day_after_the_gap() {
        // Santiago moved its clocks from 00:00 to 01:00 on 2023-09-03.
        assert_eq!(
            due_instant(day(2023, 9, 2), chrono_tz::America::Santiago),
            Utc.with_ymd_and_hms(2023, 9, 3, 4, 0, 0).unwrap()
        );
    }
}
