//! A ticket's dates: how a client writes them and what moment they name.
//!
//! `tickets.due_date` and `start_date` are floating calendar days, stored as
//! midnight UTC: "21 November" means the 21st for every viewer. `due_time`
//! and `start_time` are floating wall-clock times of that day. Every write
//! path (sync push, REST, ticket create) reads a date through [`parse_day`],
//! so `2026-11-21`, `2026-11-21T00:00:00` and `2026-11-21T00:00:00+11:00`
//! all mean the 21st.
//!
//! When the server needs a real moment (the `until_due_at` rule clock, a
//! rule condition on the due date), a day and its time resolve in the
//! workspace's timezone (`site_settings.default_timezone`, else UTC), the
//! same moment for everyone. This module is the one place those rules live.

use chrono::{DateTime, Days, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use chrono_tz::Tz;

use crate::utils::locale::{day_start, local_moment};

/// The calendar day a client wrote, as the midnight it is stored at. Takes a
/// date (`2026-11-21`), a date and time with no zone (`2026-11-21T00:00:00`)
/// or an RFC 3339 timestamp (`2026-11-21T00:00:00+11:00`); in each the day
/// is the one written, any time or offset ignored. `None` for anything else.
pub fn parse_day(raw: &str) -> Option<NaiveDateTime> {
    let raw = raw.trim();
    let day = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .or_else(|| {
            DateTime::parse_from_rfc3339(raw)
                .ok()
                .map(|t| t.naive_local().date())
        })
        .or_else(|| {
            NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|t| t.date())
        })?;
    Some(day.and_time(NaiveTime::MIN))
}

/// A time of day as a client writes it: `14:30` or `14:30:00`. `None` for
/// anything else, fractions of a second included.
pub fn parse_time(raw: &str) -> Option<NaiveTime> {
    let raw = raw.trim();
    NaiveTime::parse_from_str(raw, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(raw, "%H:%M:%S"))
        .ok()
}

/// The day a ticket is planned for: its start date when it has one,
/// otherwise its due date.
pub fn plan_day(
    start_date: Option<NaiveDateTime>,
    due_date: Option<NaiveDateTime>,
) -> Option<NaiveDate> {
    start_date.or(due_date).map(|d| d.date())
}

/// When a ticket due on `due_date` is due, in the workspace timezone `tz`:
/// `due_time` on that day, or with no time the end of the day, that is the
/// first moment of the next day. Only the calendar day of `due_date` is read.
pub fn due_instant(due_date: DateTime<Utc>, due_time: Option<NaiveTime>, tz: Tz) -> DateTime<Utc> {
    let day = due_date.date_naive();
    match due_time {
        Some(time) => local_moment(day.and_time(time), tz),
        None => day_start(
            day.checked_add_days(Days::new(1)).unwrap_or(NaiveDate::MAX),
            tz,
        ),
    }
}

/// When a ticket planned to start on `start_date` starts, in the workspace
/// timezone `tz`: `start_time` on that day, or with no time the start of
/// the day.
pub fn start_instant(
    start_date: DateTime<Utc>,
    start_time: Option<NaiveTime>,
    tz: Tz,
) -> DateTime<Utc> {
    let day = start_date.date_naive();
    match start_time {
        Some(time) => local_moment(day.and_time(time), tz),
        None => day_start(day, tz),
    }
}

/// Serde for a ticket's dates and times on the write paths: a date goes
/// through [`parse_day`], a time through [`parse_time`]. The `double_option`
/// forms are for a partial update, where an absent key leaves the column
/// alone and `null` clears it (see `utils::double_option`).
pub mod serde_plan {
    use chrono::{NaiveDateTime, NaiveTime};
    use serde::de::Error;
    use serde::{Deserialize, Deserializer};

    fn day<E: Error>(raw: Option<String>) -> Result<Option<NaiveDateTime>, E> {
        raw.map(|s| {
            super::parse_day(&s)
                .ok_or_else(|| E::custom(format!("'{s}' is not a date; give YYYY-MM-DD")))
        })
        .transpose()
    }

    fn time<E: Error>(raw: Option<String>) -> Result<Option<NaiveTime>, E> {
        raw.map(|s| {
            super::parse_time(&s)
                .ok_or_else(|| E::custom(format!("'{s}' is not a time of day; give HH:MM")))
        })
        .transpose()
    }

    pub fn day_option<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NaiveDateTime>, D::Error> {
        day(Option::<String>::deserialize(d)?)
    }

    pub fn day_double_option<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Option<NaiveDateTime>>, D::Error> {
        day(Option::<String>::deserialize(d)?).map(Some)
    }

    pub fn time_option<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NaiveTime>, D::Error> {
        time(Option::<String>::deserialize(d)?)
    }

    pub fn time_double_option<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Option<NaiveTime>>, D::Error> {
        time(Option::<String>::deserialize(d)?).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn day(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
    }

    fn hm(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn a_due_day_ends_at_the_workspace_midnight() {
        // Sydney in November is UTC+11: the 21st ends at 13:00 UTC on the 21st.
        assert_eq!(
            due_instant(day(2026, 11, 21), None, chrono_tz::Australia::Sydney),
            Utc.with_ymd_and_hms(2026, 11, 21, 13, 0, 0).unwrap()
        );
        // New York in November is UTC-5: the 21st ends at 05:00 UTC on the 22nd.
        assert_eq!(
            due_instant(day(2026, 11, 21), None, chrono_tz::America::New_York),
            Utc.with_ymd_and_hms(2026, 11, 22, 5, 0, 0).unwrap()
        );
        assert_eq!(
            due_instant(day(2026, 11, 21), None, Tz::UTC),
            day(2026, 11, 22)
        );
    }

    #[test]
    fn a_due_time_is_that_moment_in_the_workspace_zone() {
        // 14:30 in Sydney (UTC+11) is 03:30 UTC the same day.
        assert_eq!(
            due_instant(
                day(2026, 11, 21),
                Some(hm(14, 30)),
                chrono_tz::Australia::Sydney
            ),
            Utc.with_ymd_and_hms(2026, 11, 21, 3, 30, 0).unwrap()
        );
        // Midnight is the start of the day, not its end.
        assert_eq!(
            due_instant(day(2026, 11, 21), Some(hm(0, 0)), Tz::UTC),
            day(2026, 11, 21)
        );
    }

    #[test]
    fn a_start_is_the_start_of_its_day_or_its_time() {
        assert_eq!(
            start_instant(day(2026, 11, 21), None, chrono_tz::America::New_York),
            Utc.with_ymd_and_hms(2026, 11, 21, 5, 0, 0).unwrap()
        );
        assert_eq!(
            start_instant(
                day(2026, 11, 21),
                Some(hm(9, 15)),
                chrono_tz::America::New_York
            ),
            Utc.with_ymd_and_hms(2026, 11, 21, 14, 15, 0).unwrap()
        );
    }

    #[test]
    fn only_the_calendar_day_is_read() {
        let stray = Utc.with_ymd_and_hms(2026, 11, 21, 17, 42, 0).unwrap();
        assert_eq!(due_instant(stray, None, Tz::UTC), day(2026, 11, 22));
    }

    #[test]
    fn a_zone_that_skips_midnight_starts_the_day_after_the_gap() {
        // Santiago moved its clocks from 00:00 to 01:00 on 2023-09-03.
        assert_eq!(
            due_instant(day(2023, 9, 2), None, chrono_tz::America::Santiago),
            Utc.with_ymd_and_hms(2023, 9, 3, 4, 0, 0).unwrap()
        );
    }

    #[test]
    fn a_time_the_clocks_skip_reads_as_just_after_the_gap() {
        // New York skipped 02:00 to 03:00 on 2026-03-08; 02:30 reads as 03:30
        // EDT, 07:30 UTC.
        assert_eq!(
            due_instant(
                day(2026, 3, 8),
                Some(hm(2, 30)),
                chrono_tz::America::New_York
            ),
            Utc.with_ymd_and_hms(2026, 3, 8, 7, 30, 0).unwrap()
        );
    }

    #[test]
    fn every_write_shape_of_a_day_is_that_day() {
        let the_21st = NaiveDate::from_ymd_opt(2026, 11, 21)
            .unwrap()
            .and_time(NaiveTime::MIN);
        for raw in [
            "2026-11-21",
            "2026-11-21T00:00:00",
            "2026-11-21T00:00:00.000",
            "2026-11-21T00:00:00+11:00",
            "2026-11-21T23:30:00-05:00",
            "2026-11-21T09:12:00Z",
            " 2026-11-21 ",
        ] {
            assert_eq!(parse_day(raw), Some(the_21st), "{raw}");
        }
        for raw in [
            "",
            "tomorrow",
            "2026-13-01",
            "21/11/2026",
            "2026-11-21T25:00:00",
        ] {
            assert_eq!(parse_day(raw), None, "{raw}");
        }
    }

    #[test]
    fn a_time_is_hours_and_minutes() {
        assert_eq!(parse_time("14:30"), Some(hm(14, 30)));
        assert_eq!(parse_time("14:30:00"), Some(hm(14, 30)));
        assert_eq!(parse_time("00:00"), Some(hm(0, 0)));
        for raw in ["", "2pm", "24:00", "14:30:00.5", "14"] {
            assert_eq!(parse_time(raw), None, "{raw}");
        }
    }

    #[test]
    fn the_plan_day_is_the_start_then_the_due_date() {
        let d = |n| {
            NaiveDate::from_ymd_opt(2026, 11, n)
                .unwrap()
                .and_time(NaiveTime::MIN)
        };
        assert_eq!(plan_day(Some(d(3)), Some(d(9))), Some(d(3).date()));
        assert_eq!(plan_day(None, Some(d(9))), Some(d(9).date()));
        assert_eq!(plan_day(None, None), None);
    }
}
