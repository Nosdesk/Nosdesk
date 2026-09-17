/**
 * Calendar arithmetic over JS `Date`s, on `@internationalized/date`.
 *
 * Every helper takes the IANA zone the calendar is reckoned in, defaulting
 * to the browser's. That default is what the Gantt wants: its dates are
 * local midnights and a bar stays on the day the user dropped it on.
 * Anything anchored to the user's configured zone passes
 * `dateStore.effectiveTimezone` explicitly. Formatting lives in
 * `dateUtils`; this module never produces user-facing text.
 *
 * Two shapes, deliberately:
 *  - Calendar boundaries (start of day/week/month/quarter, day strings,
 *    day counts) round-trip through `CalendarDate`, so the result is
 *    midnight in `tz` even where midnight does not exist (a DST gap
 *    resolves forward, 01:00 local, same calendar day).
 *  - Shifts (`addDays` and friends) go through `ZonedDateTime`, so the
 *    wall-clock time survives a DST transition: 09:00 plus a day is 09:00.
 */

import {
  CalendarDate,
  fromDate,
  getLocalTimeZone,
  parseDate as parseCalendarDate,
  startOfMonth as calendarStartOfMonth,
  startOfWeek as calendarStartOfWeek,
  toCalendarDate,
  type DateDuration,
} from '@internationalized/date'

export { getLocalTimeZone }

/** The calendar day `d` falls on in `tz`. */
export function calendarDayOf(d: Date, tz = getLocalTimeZone()): CalendarDate {
  return toCalendarDate(fromDate(d, tz))
}

function shift(d: Date, duration: DateDuration, tz: string): Date {
  return fromDate(d, tz).add(duration).toDate()
}

export function addDays(d: Date, days: number, tz = getLocalTimeZone()): Date {
  return shift(d, { days }, tz)
}

export function addWeeks(d: Date, weeks: number, tz = getLocalTimeZone()): Date {
  return shift(d, { weeks }, tz)
}

/** Day-of-month is constrained: Jan 31 plus a month is Feb 28. */
export function addMonths(d: Date, months: number, tz = getLocalTimeZone()): Date {
  return shift(d, { months }, tz)
}

export function addYears(d: Date, years: number, tz = getLocalTimeZone()): Date {
  return shift(d, { years }, tz)
}

export function startOfDay(d: Date, tz = getLocalTimeZone()): Date {
  return calendarDayOf(d, tz).toDate(tz)
}

/** Last millisecond of the day, so a `to` bound is inclusive of it. */
export function endOfDay(d: Date, tz = getLocalTimeZone()): Date {
  return new Date(calendarDayOf(d, tz).add({ days: 1 }).toDate(tz).getTime() - 1)
}

/** Monday-based, as the Gantt rulers and the backend's week buckets are. */
export function startOfWeek(d: Date, tz = getLocalTimeZone()): Date {
  return calendarStartOfWeek(calendarDayOf(d, tz), 'en-US', 'mon').toDate(tz)
}

export function startOfMonth(d: Date, tz = getLocalTimeZone()): Date {
  return calendarStartOfMonth(calendarDayOf(d, tz)).toDate(tz)
}

export function startOfQuarter(d: Date, tz = getLocalTimeZone()): Date {
  const day = calendarDayOf(d, tz)
  return new CalendarDate(day.year, day.month - ((day.month - 1) % 3), 1).toDate(tz)
}

/** Whole calendar days from `from` to `to` in `tz`; negative when `to` is
 *  earlier. Time of day is ignored: 23:59 to 00:01 is one day. */
export function daysBetween(from: Date, to: Date, tz = getLocalTimeZone()): number {
  return calendarDayOf(to, tz).compare(calendarDayOf(from, tz))
}

/** `YYYY-MM-DD` of the day `d` falls on in `tz`. Machine-facing (API
 *  payloads, lane keys); user-facing dates go through `dateUtils`. */
export function toCalendarDateString(d: Date, tz = getLocalTimeZone()): string {
  return calendarDayOf(d, tz).toString()
}

/** Midnight in `tz` of a `YYYY-MM-DD` string (longer input is truncated);
 *  `null` when it is not a real date. */
export function fromCalendarDateString(value: string, tz = getLocalTimeZone()): Date | null {
  try {
    return parseCalendarDate(value.slice(0, 10)).toDate(tz)
  } catch {
    return null
  }
}
