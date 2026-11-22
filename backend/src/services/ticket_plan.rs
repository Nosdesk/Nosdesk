//! The rules a ticket's plan keeps on every write (docs/plans/work-planning):
//! its due and start times, its place in a day (`rank`) and its size
//! (`estimate_minutes`).
//!
//! The ticket update applies [`apply_to_update`] under the ticket's lock, so
//! sync push, REST and rules write the plan the same way and an old app that
//! knows nothing of it can't leave it inconsistent:
//!
//! - A time needs its date, and clearing a date clears its time.
//! - The place in a day belongs to that day and that person. A write that
//!   moves the ticket to another plan day ([`ticket_dates::plan_day`]) or
//!   gives it to someone else without placing it clears `rank`, so it arrives
//!   unplaced. Closing it, or setting a time, keeps it.
//! - A rank is a fractional index key ([`check_rank`]); a size is a number
//!   of minutes in a sane range ([`check_estimate`]).
//!
//! A new ticket has no rank: it lands unplaced. [`check_new`] holds a new
//! ticket's times and size to the same rules.

use crate::models::{NewTicket, Ticket, TicketUpdate};
use crate::services::ticket_dates;

/// The longest job a size can describe: a working week of 24-hour days. A
/// bigger value is a mistake, not a plan.
pub const MAX_ESTIMATE_MINUTES: i32 = 7 * 24 * 60;

/// The longest rank key. A fractional index grows by about a character each
/// time a job is dropped between the same two neighbours; this is far past
/// anything a hand-ordered day reaches.
pub const MAX_RANK_LEN: usize = 128;

/// Why a write's plan fields were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("a due time needs a due date")]
    DueTimeWithoutDate,
    #[error("a start time needs a start date")]
    StartTimeWithoutDate,
    #[error("an estimate is a whole number of minutes from 1 to {MAX_ESTIMATE_MINUTES}")]
    Estimate,
    #[error("a rank is 1 to {MAX_RANK_LEN} letters and digits")]
    Rank,
}

/// A rank key: ASCII letters and digits, as fractional index keys are
/// written, so byte order (`COLLATE "C"`) is the order the client meant.
pub fn check_rank(rank: &str) -> Result<(), PlanError> {
    if rank.is_empty()
        || rank.len() > MAX_RANK_LEN
        || !rank.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err(PlanError::Rank);
    }
    Ok(())
}

pub fn check_estimate(minutes: i32) -> Result<(), PlanError> {
    if (1..=MAX_ESTIMATE_MINUTES).contains(&minutes) {
        Ok(())
    } else {
        Err(PlanError::Estimate)
    }
}

/// A new ticket's plan fields hold to the rules an update does.
pub fn check_new(ticket: &NewTicket) -> Result<(), PlanError> {
    if ticket.due_time.is_some() && ticket.due_date.is_none() {
        return Err(PlanError::DueTimeWithoutDate);
    }
    if ticket.start_time.is_some() && ticket.start_date.is_none() {
        return Err(PlanError::StartTimeWithoutDate);
    }
    ticket.estimate_minutes.map(check_estimate).transpose()?;
    Ok(())
}

/// Check `update`'s plan fields against the ticket as it stands (`before`)
/// and add what follows from them: the time a cleared date takes with it,
/// and a cleared rank when the ticket leaves its day or its person.
pub fn apply_to_update(update: &mut TicketUpdate, before: &Ticket) -> Result<(), PlanError> {
    if let Some(Some(rank)) = &update.rank {
        check_rank(rank)?;
    }
    if let Some(Some(minutes)) = update.estimate_minutes {
        check_estimate(minutes)?;
    }

    // Clearing a date clears its time.
    if update.due_date == Some(None) && update.due_time.is_none() && before.due_time.is_some() {
        update.due_time = Some(None);
    }
    if update.start_date == Some(None) && update.start_time.is_none() && before.start_time.is_some()
    {
        update.start_time = Some(None);
    }
    let due_date = update.due_date.unwrap_or(before.due_date);
    let start_date = update.start_date.unwrap_or(before.start_date);
    if update.due_time.unwrap_or(before.due_time).is_some() && due_date.is_none() {
        return Err(PlanError::DueTimeWithoutDate);
    }
    if update.start_time.unwrap_or(before.start_time).is_some() && start_date.is_none() {
        return Err(PlanError::StartTimeWithoutDate);
    }

    // A place in one day, or in one person's day, means nothing in another.
    if update.rank.is_none() && before.rank.is_some() {
        let moved_day = ticket_dates::plan_day(start_date, due_date)
            != ticket_dates::plan_day(before.start_date, before.due_date);
        let reassigned = update
            .assignee_uuid
            .is_some_and(|assignee| assignee != before.assignee_uuid);
        if moved_day || reassigned {
            update.rank = Some(None);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
    use uuid::Uuid;

    fn day(n: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, n)
            .unwrap()
            .and_time(NaiveTime::MIN)
    }

    fn hm(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    fn placed() -> Ticket {
        let mut t = crate::models::test_ticket();
        t.start_date = None;
        t.due_date = Some(day(21));
        t.due_time = Some(hm(14, 30));
        t.rank = Some("a0".into());
        t.assignee_uuid = Some(Uuid::nil());
        t
    }

    #[test]
    fn a_rank_is_letters_and_digits() {
        assert_eq!(check_rank("a0"), Ok(()));
        assert_eq!(check_rank("Zz09V"), Ok(()));
        for bad in ["", "a 0", "a-0", "é", &"a".repeat(MAX_RANK_LEN + 1)] {
            assert_eq!(check_rank(bad), Err(PlanError::Rank), "{bad}");
        }
    }

    #[test]
    fn an_estimate_is_in_range() {
        assert_eq!(check_estimate(30), Ok(()));
        assert_eq!(check_estimate(MAX_ESTIMATE_MINUTES), Ok(()));
        for bad in [0, -5, MAX_ESTIMATE_MINUTES + 1] {
            assert_eq!(check_estimate(bad), Err(PlanError::Estimate), "{bad}");
        }
    }

    #[test]
    fn clearing_a_date_clears_its_time() {
        let mut update = TicketUpdate {
            due_date: Some(None),
            ..TicketUpdate::default()
        };
        apply_to_update(&mut update, &placed()).unwrap();
        assert_eq!(update.due_time, Some(None));
        // It also leaves the day, so its place goes.
        assert_eq!(update.rank, Some(None));
    }

    #[test]
    fn a_time_needs_its_date() {
        let mut no_due = placed();
        no_due.due_date = None;
        no_due.due_time = None;
        let mut update = TicketUpdate {
            due_time: Some(Some(hm(9, 0))),
            ..TicketUpdate::default()
        };
        assert_eq!(
            apply_to_update(&mut update, &no_due),
            Err(PlanError::DueTimeWithoutDate)
        );
        let mut update = TicketUpdate {
            due_date: Some(None),
            due_time: Some(Some(hm(9, 0))),
            ..TicketUpdate::default()
        };
        assert_eq!(
            apply_to_update(&mut update, &placed()),
            Err(PlanError::DueTimeWithoutDate)
        );
        let mut update = TicketUpdate {
            start_time: Some(Some(hm(9, 0))),
            ..TicketUpdate::default()
        };
        assert_eq!(
            apply_to_update(&mut update, &placed()),
            Err(PlanError::StartTimeWithoutDate)
        );
    }

    #[test]
    fn moving_to_another_day_without_a_place_clears_the_rank() {
        let mut update = TicketUpdate {
            due_date: Some(Some(day(22))),
            ..TicketUpdate::default()
        };
        apply_to_update(&mut update, &placed()).unwrap();
        assert_eq!(update.rank, Some(None));

        // A drop on another day places it there.
        let mut update = TicketUpdate {
            due_date: Some(Some(day(22))),
            rank: Some(Some("a1".into())),
            ..TicketUpdate::default()
        };
        apply_to_update(&mut update, &placed()).unwrap();
        assert_eq!(update.rank, Some(Some("a1".into())));
    }

    #[test]
    fn a_deadline_edit_under_a_start_date_keeps_the_place() {
        let mut planned = placed();
        planned.start_date = Some(day(20));
        let mut update = TicketUpdate {
            due_date: Some(Some(day(28))),
            ..TicketUpdate::default()
        };
        apply_to_update(&mut update, &planned).unwrap();
        assert_eq!(update.rank, None);

        // Setting a start date on another day moves the plan day.
        let mut update = TicketUpdate {
            start_date: Some(Some(day(19))),
            ..TicketUpdate::default()
        };
        apply_to_update(&mut update, &planned).unwrap();
        assert_eq!(update.rank, Some(None));
    }

    #[test]
    fn giving_it_to_someone_else_clears_the_rank() {
        let mut update = TicketUpdate {
            assignee_uuid: Some(Some(Uuid::now_v7())),
            ..TicketUpdate::default()
        };
        apply_to_update(&mut update, &placed()).unwrap();
        assert_eq!(update.rank, Some(None));

        // Naming the same assignee is no reassignment.
        let mut update = TicketUpdate {
            assignee_uuid: Some(Some(Uuid::nil())),
            ..TicketUpdate::default()
        };
        apply_to_update(&mut update, &placed()).unwrap();
        assert_eq!(update.rank, None);
    }

    #[test]
    fn a_time_or_a_close_keeps_the_place() {
        for mut update in [
            TicketUpdate {
                due_time: Some(None),
                ..TicketUpdate::default()
            },
            TicketUpdate {
                due_time: Some(Some(hm(8, 0))),
                ..TicketUpdate::default()
            },
            TicketUpdate {
                workflow_state_id: Some(99),
                ..TicketUpdate::default()
            },
            TicketUpdate {
                due_date: Some(Some(day(21))),
                ..TicketUpdate::default()
            },
        ] {
            apply_to_update(&mut update, &placed()).unwrap();
            assert_eq!(update.rank, None, "{update:?}");
        }
    }

    #[test]
    fn a_new_ticket_holds_to_the_same_rules() {
        let ok = NewTicket {
            due_date: Some(day(21)),
            due_time: Some(hm(9, 0)),
            estimate_minutes: Some(120),
            ..NewTicket::default()
        };
        assert_eq!(check_new(&ok), Ok(()));
        let timeless = NewTicket {
            start_time: Some(hm(9, 0)),
            ..NewTicket::default()
        };
        assert_eq!(check_new(&timeless), Err(PlanError::StartTimeWithoutDate));
        let huge = NewTicket {
            estimate_minutes: Some(MAX_ESTIMATE_MINUTES + 1),
            ..NewTicket::default()
        };
        assert_eq!(check_new(&huge), Err(PlanError::Estimate));
    }
}
