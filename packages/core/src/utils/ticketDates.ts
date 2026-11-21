/**
 * A ticket's dates, read one way everywhere.
 *
 * `due_date` and `start_date` are floating calendar days: "21 November"
 * means the 21st for every viewer. The app stores midnight with no zone,
 * but other writers have left offsets and stray times of day, so a reader
 * takes the date part as written and never shifts it through a zone.
 * `2026-11-21`, `2026-11-21T00:00:00` and `2026-11-21T09:30:00+10:00` are
 * all the 21st.
 *
 * When a day has to become a moment ("is this overdue?") it resolves in
 * the planning timezone, and a due day lasts until the first moment of the
 * next day there. The backend twin is `services::ticket_dates::due_instant`,
 * which resolves in the workspace timezone. `planningTimezone()` is the
 * `effective_timezone` `/auth/me` returns (the user's preference, else the
 * workspace default, else UTC), so for everyone without a preference of
 * their own it is the workspace's zone and agrees with the server. Before
 * sign-in, or from a server that doesn't send it, it is the browser's zone.
 * Display formatting keeps its own zone (`dateStore.effectiveTimezone`).
 *
 * "Today" is a `YYYY-MM-DD` in the same zone, so a day compares to it as a
 * string and the due buckets partition cleanly: a day before today is
 * overdue, and nothing else is.
 */
import { CalendarDate, parseDate as parseCalendarDate } from '@internationalized/date'
import { formatDate } from './dateUtils'
import { getLocalTimeZone, toCalendarDateString } from './dateMath'

/** A calendar day as `YYYY-MM-DD`. */
export type Day = string

export interface TicketDates {
  due_date?: string | null
  start_date?: string | null
}

function toCalendar(day: Day): CalendarDate | null {
  try {
    return parseCalendarDate(day)
  } catch {
    return null
  }
}

/** The calendar day a stored date names: its date part as written. `null`
 *  for an empty or unreadable value. */
export function floatingDay(value: string | null | undefined): Day | null {
  if (!value) return null
  const match = /^\d{4}-\d{2}-\d{2}/.exec(value)
  if (!match) return null
  return toCalendar(match[0])?.toString() ?? null
}

/** The deadline day. */
export function dueDay(ticket: TicketDates): Day | null {
  return floatingDay(ticket.due_date)
}

/** The planned day, when one is set apart from the deadline. */
export function startDay(ticket: TicketDates): Day | null {
  return floatingDay(ticket.start_date)
}

/** The day the work is planned for: the start date when set, else the
 *  due date. */
export function planDay(ticket: TicketDates): Day | null {
  return startDay(ticket) ?? dueDay(ticket)
}

let serverTimezone: string | null = null

function isZone(tz: string): boolean {
  try {
    new Intl.DateTimeFormat('en-US', { timeZone: tz })
    return true
  } catch {
    return false
  }
}

/** Record the server-resolved zone from `/auth/me` (`effective_timezone`).
 *  `null`, or a zone this browser doesn't know, falls back to the
 *  browser's. */
export function setPlanningTimezone(tz: string | null | undefined): void {
  serverTimezone = tz && isZone(tz) ? tz : null
}

/** The zone days resolve in: the server-resolved zone, else the
 *  browser's. */
export function planningTimezone(): string {
  return serverTimezone ?? getLocalTimeZone()
}

/** Today in `tz`. */
export function todayIn(tz: string = planningTimezone(), now: Date = new Date()): Day {
  return toCalendarDateString(now, tz)
}

/** Whole days from `from` to `to`; negative when `to` is earlier. */
export function daysBetweenDays(from: Day, to: Day): number {
  const a = toCalendar(from)
  const b = toCalendar(to)
  if (!a || !b) return 0
  return b.compare(a)
}

/** When a ticket due on `day` is due: the end of that day, the first
 *  moment of the next, in `tz`. A zone that skips midnight starts the day
 *  at its first moment after the gap, as the backend does. */
export function dueInstant(day: Day, tz: string = planningTimezone()): Date | null {
  return toCalendar(day)?.add({ days: 1 }).toDate(tz) ?? null
}

/** Past the end of the due day. A ticket due today is not overdue until
 *  tomorrow; one with no due date never is. */
export function isOverdue(
  ticket: TicketDates,
  now: Date = new Date(),
  tz: string = planningTimezone(),
): boolean {
  const day = dueDay(ticket)
  if (!day) return false
  const instant = dueInstant(day, tz)
  return instant != null && now.getTime() >= instant.getTime()
}

// ---------------------------------------------------------------
// Due buckets
// ---------------------------------------------------------------

export type DueBucket = 'overdue' | 'today' | 'tomorrow' | 'this-week' | 'later' | 'none'

/** Display order of the buckets. */
export const DUE_BUCKETS: readonly DueBucket[] = [
  'overdue',
  'today',
  'tomorrow',
  'this-week',
  'later',
  'none',
]

/** Which bucket a ticket's due day falls in, relative to `today`. "This
 *  week" runs to Sunday (weeks start on Monday, as the gantt's and the
 *  backend's do), so late in the week it can be empty. */
export function dueBucket(ticket: TicketDates, today: Day): DueBucket {
  const day = dueDay(ticket)
  if (!day) return 'none'
  const diff = daysBetweenDays(today, day)
  if (diff < 0) return 'overdue'
  if (diff === 0) return 'today'
  if (diff === 1) return 'tomorrow'
  const start = toCalendar(today)
  if (!start) return 'later'
  // 0 for Monday through 6 for Sunday.
  const weekday = (start.toDate('UTC').getUTCDay() + 6) % 7
  return diff <= 6 - weekday ? 'this-week' : 'later'
}

// ---------------------------------------------------------------
// Display
// ---------------------------------------------------------------

/** A day in the active locale, never shifted by a zone. */
export function formatDay(day: Day, preset: Parameters<typeof formatDate>[1] = 'date'): string {
  return formatDate(`${day}T00:00:00Z`, preset, 'UTC')
}

/** "Nov 21", with the year only when it is not this year's. */
export function formatCompactDay(day: Day, today: Day = todayIn()): string {
  return formatDay(day, day.slice(0, 4) === today.slice(0, 4) ? 'monthDay' : 'date')
}

/** The due chip's wording: "Today", "Tomorrow", the weekday and date
 *  within a week either side ("Fri 27"), else the compact date. */
export function dueChipLabel(
  day: Day,
  today: Day,
  words: { today: string; tomorrow: string },
): string {
  const diff = daysBetweenDays(today, day)
  if (diff === 0) return words.today
  if (diff === 1) return words.tomorrow
  if (Math.abs(diff) <= 6) return formatDay(day, 'weekdayDay')
  return formatCompactDay(day, today)
}
