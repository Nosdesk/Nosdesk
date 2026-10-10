//! Recurring ticket materialisation.
//!
//! When a ticket carrying `recurrence_rule` is closed, the next
//! occurrence is generated synchronously rather than by a periodic
//! job. This keeps the lifecycle local: the user closes a ticket,
//! the next instance shows up immediately, no scheduler tax to
//! reason about.
//!
//! The rule is an RFC 5545 RRULE without a DTSTART line — DTSTART
//! is implicit (the closed ticket's due_date or, lacking that, its
//! created_at). The crate's `RRule` builder handles parsing and
//! the next-instance lookup.

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Utc};
use rrule::{RRule, RRuleSet, Tz, Unvalidated, Validated};

/// Returns the next occurrence after `after`, or `None` if the rule
/// has no further dates (e.g. UNTIL has passed). Errors out on
/// parse failure so the caller can decide whether to surface or
/// log; we never want a malformed rule to brick close.
pub fn next_occurrence(
    rule: &str,
    series_start: DateTime<Utc>,
    after: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, RecurrenceError> {
    // The RRule crate operates on its own Tz wrapper; we feed it
    // UTC because Nosdesk persists wall-clock-as-UTC across the
    // tickets table.
    let dtstart = series_start.with_timezone(&Tz::UTC);
    let unvalidated: RRule<Unvalidated> = rule
        .parse::<RRule<Unvalidated>>()
        .map_err(|e| RecurrenceError::Parse(e.to_string()))?;
    let validated: RRule<Validated> = unvalidated
        .validate(dtstart)
        .map_err(|e| RecurrenceError::Parse(e.to_string()))?;
    let mut set = RRuleSet::new(dtstart);
    set = set.rrule(validated);

    // Generate up to a small window after the cutoff and return
    // the first one strictly after `after`. The crate's iterator
    // is unbounded for forever-recurring rules; cap at a reasonable
    // ceiling so a malformed rule with a very late DTSTART can't
    // burn CPU.
    const MAX_OCCURRENCES: usize = 1024;
    let after_tz = after.with_timezone(&Tz::UTC);
    let mut iter = set.into_iter();
    for _ in 0..MAX_OCCURRENCES {
        match iter.next() {
            Some(dt) if dt > after_tz => return Ok(Some(dt.with_timezone(&Utc))),
            Some(_) => continue,
            None => return Ok(None),
        }
    }
    Ok(None)
}

/// Same as `next_occurrence` but operates on the NaiveDateTime
/// shape the Ticket model uses.
pub fn next_occurrence_naive(
    rule: &str,
    series_start: NaiveDateTime,
    after: NaiveDateTime,
) -> Result<Option<NaiveDateTime>, RecurrenceError> {
    let series_utc = DateTime::<Utc>::from_naive_utc_and_offset(series_start, Utc);
    let after_utc = DateTime::<Utc>::from_naive_utc_and_offset(after, Utc);
    next_occurrence(rule, series_utc, after_utc).map(|opt| opt.map(|dt| dt.naive_utc()))
}

/// `rule` made to keep the series' day of the month near a month end.
///
/// RFC 5545 drops a date a month doesn't have, so a plain monthly series
/// on the 31st skips every shorter month, and a yearly one on 29 February
/// skips three years in four. For a plain `FREQ=MONTHLY` or `FREQ=YEARLY`
/// rule (no `BY` parts of its own) whose `anchor` (the series' first due
/// date) falls on the 29th or later, this adds `BYMONTHDAY=<day>,-1;
/// BYSETPOS=1`: the anchor's day, or the month's last day when the month
/// is shorter (and `BYMONTH` for a yearly rule). Jan 31 then gives Feb 28
/// (29 in a leap year) and Mar 31, rather than drifting to the 28th. Any
/// other rule is returned as is.
pub fn keep_month_end_day(rule: &str, anchor: NaiveDate) -> String {
    let parts: Vec<&str> = rule.split(';').filter(|p| !p.is_empty()).collect();
    let freq = parts
        .iter()
        .find_map(|p| p.strip_prefix("FREQ="))
        .map(str::to_ascii_uppercase);
    let has_by = parts
        .iter()
        .any(|p| p.get(..2).is_some_and(|by| by.eq_ignore_ascii_case("BY")));
    let day = anchor.day();
    if has_by || day < 29 {
        return rule.to_string();
    }
    match freq.as_deref() {
        Some("MONTHLY") => format!("{rule};BYMONTHDAY={day},-1;BYSETPOS=1"),
        Some("YEARLY") => format!(
            "{rule};BYMONTH={};BYMONTHDAY={day},-1;BYSETPOS=1",
            anchor.month()
        ),
        _ => rule.to_string(),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RecurrenceError {
    #[error("RRULE parse error: {0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn weekly_monday_after_a_friday_lands_on_next_monday() {
        let start = Utc.with_ymd_and_hms(2026, 1, 5, 9, 0, 0).unwrap(); // Mon Jan 5
        let after = Utc.with_ymd_and_hms(2026, 1, 9, 17, 0, 0).unwrap(); // Fri Jan 9
        let next = next_occurrence("FREQ=WEEKLY;BYDAY=MO", start, after).unwrap();
        assert_eq!(
            next,
            Some(Utc.with_ymd_and_hms(2026, 1, 12, 9, 0, 0).unwrap())
        );
    }

    /// The due dates a series anchored at `first` (midnight) gets, closing
    /// each occurrence in turn, as `create_next_occurrence` does.
    fn series(rule: &str, first: NaiveDate, n: usize) -> Vec<NaiveDate> {
        let rule = keep_month_end_day(rule, first);
        let mut due = first.and_hms_opt(0, 0, 0).unwrap();
        (0..n)
            .map(|_| {
                due = next_occurrence_naive(&rule, due, due).unwrap().unwrap();
                due.date()
            })
            .collect()
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn monthly_on_the_31st_keeps_its_day_through_short_months() {
        assert_eq!(
            series("FREQ=MONTHLY", day(2027, 1, 31), 4),
            vec![
                day(2027, 2, 28),
                day(2027, 3, 31),
                day(2027, 4, 30),
                day(2027, 5, 31)
            ]
        );
    }

    #[test]
    fn monthly_on_the_31st_lands_on_29_february_in_a_leap_year() {
        assert_eq!(
            series("FREQ=MONTHLY", day(2028, 1, 31), 2),
            vec![day(2028, 2, 29), day(2028, 3, 31)]
        );
    }

    #[test]
    fn monthly_on_the_30th_keeps_the_30th_in_long_months() {
        assert_eq!(
            series("FREQ=MONTHLY", day(2027, 1, 30), 3),
            vec![day(2027, 2, 28), day(2027, 3, 30), day(2027, 4, 30)]
        );
    }

    #[test]
    fn yearly_on_29_february_lands_on_the_28th_until_a_leap_year() {
        assert_eq!(
            series("FREQ=YEARLY", day(2028, 2, 29), 4),
            vec![
                day(2029, 2, 28),
                day(2030, 2, 28),
                day(2031, 2, 28),
                day(2032, 2, 29)
            ]
        );
    }

    #[test]
    fn early_days_and_rules_with_their_own_by_parts_are_left_alone() {
        assert_eq!(
            keep_month_end_day("FREQ=MONTHLY", day(2027, 1, 15)),
            "FREQ=MONTHLY"
        );
        assert_eq!(
            keep_month_end_day("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR", day(2027, 1, 29)),
            "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"
        );
        assert_eq!(
            keep_month_end_day("FREQ=MONTHLY;BYMONTHDAY=-1", day(2027, 1, 31)),
            "FREQ=MONTHLY;BYMONTHDAY=-1"
        );
        assert_eq!(
            keep_month_end_day("FREQ=DAILY", day(2027, 1, 31)),
            "FREQ=DAILY"
        );
    }

    #[test]
    fn until_clause_terminates_series() {
        let start = Utc.with_ymd_and_hms(2026, 1, 5, 9, 0, 0).unwrap();
        let after = Utc.with_ymd_and_hms(2026, 2, 15, 0, 0, 0).unwrap();
        let next =
            next_occurrence("FREQ=WEEKLY;BYDAY=MO;UNTIL=20260131T000000Z", start, after).unwrap();
        assert!(next.is_none());
    }

    #[test]
    fn malformed_rule_returns_parse_error() {
        let start = Utc.with_ymd_and_hms(2026, 1, 5, 9, 0, 0).unwrap();
        let result = next_occurrence("not a real rrule", start, start);
        assert!(matches!(result, Err(RecurrenceError::Parse(_))));
    }
}
