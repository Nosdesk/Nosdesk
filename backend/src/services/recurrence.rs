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

use chrono::{DateTime, NaiveDateTime, Utc};
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
