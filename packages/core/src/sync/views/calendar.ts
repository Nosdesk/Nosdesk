/**
 * The calendar, read one way: which days a month shows, which jobs sit on
 * each and in what order, what is late, which days the agenda lists, and
 * what carrying a job to another day writes. Pure functions over ticket
 * fields.
 *
 * A job sits on its plan day (the start date when set, else the due
 * date), as in My day, and a day reads in My day's order: jobs with a
 * time by time, then the rest in the day's order (`planOrder.ts`).
 *
 * Days are `YYYY-MM-DD` and formatted in the given locale with no zone
 * shift; "today" and "now" come from the planning timezone
 * (`ticketDates.ts`), never the device's.
 */
import { parseDate, startOfWeek, type CalendarDate } from '@internationalized/date'
import type { CardData } from './types'
import { compareAnyTime, compareBooked } from './planOrder'
import { toCalendarDateString } from '../../utils/dateMath'
import { dueDay, planDay, planningTimezone, type Day } from '../../utils/ticketDates'
import { planField, planTime } from '../../utils/ticketPlan'

/** A month as `YYYY-MM`. */
export type Month = string

/** The field that places a job on a day. */
export type DateField = 'due_date' | 'created_at' | 'last_activity_at'

/** How the calendar is read: a month at a time, or as one list of days. */
export type CalendarRange = 'month' | 'agenda'

export function monthOf(day: Day): Month {
  return day.slice(0, 7)
}

export function shiftMonth(month: Month, by: number): Month {
  return parseDate(`${month}-01`).add({ months: by }).toString().slice(0, 7)
}

export function shiftDay(day: Day, by: number): Day {
  return parseDate(day).add({ days: by }).toString()
}

/** The same day of the month in the month `by` away, kept inside it:
 *  31 January and one month on is 28 or 29 February. */
export function shiftDayByMonths(day: Day, by: number): Day {
  return parseDate(day).add({ months: by }).toString()
}

/** The weeks a month view shows: whole weeks from the one holding the
 *  1st to the one holding the last day, so four to six rows and never an
 *  empty one. The week starts where the locale starts it. */
export function monthWeeks(month: Month, locale: string): Day[][] {
  const first = parseDate(`${month}-01`)
  const last = first.add({ months: 1 }).subtract({ days: 1 })
  let cursor: CalendarDate = startOfWeek(first, locale)
  const weeks: Day[][] = []
  while (cursor.compare(last) <= 0) {
    const week: Day[] = []
    for (let i = 0; i < 7; i++) week.push(cursor.add({ days: i }).toString())
    weeks.push(week)
    cursor = cursor.add({ weeks: 1 })
  }
  return weeks
}

// ---------------------------------------------------------------
// Words for days
// ---------------------------------------------------------------

function utc(day: Day): Date {
  return new Date(`${day}T00:00:00Z`)
}

function formatter(locale: string, options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  return new Intl.DateTimeFormat(locale, { ...options, timeZone: 'UTC' })
}

/** "November 2026". */
export function monthTitle(month: Month, locale: string): string {
  return formatter(locale, { month: 'long', year: 'numeric' }).format(utc(`${month}-01`))
}

const DAY_TITLE: Intl.DateTimeFormatOptions = { weekday: 'long', day: 'numeric', month: 'long' }

/** "Thursday 26 November". */
export function dayTitle(day: Day, locale: string): string {
  return formatter(locale, DAY_TITLE).format(utc(day))
}

/** A day's title in pieces, with the day of the month marked, so the
 *  number alone can carry a mark: ["Thursday ", "26", " November"]. */
export function dayTitleParts(day: Day, locale: string): { text: string; number: boolean }[] {
  return formatter(locale, DAY_TITLE)
    .formatToParts(utc(day))
    .map((part) => ({ text: part.value, number: part.type === 'day' }))
}

/** "Mon" (or "M" when narrow) for a column, from any day in it. */
export function weekdayLabel(day: Day, locale: string, width: 'short' | 'narrow' | 'long' = 'short'): string {
  return formatter(locale, { weekday: width }).format(utc(day))
}

/** The number in a day cell. The first of a neighbouring month names
 *  it: "1 Dec". */
export function dayNumber(day: Day, month: Month, locale: string): string {
  if (monthOf(day) !== month && day.endsWith('-01')) {
    return formatter(locale, { day: 'numeric', month: 'short' }).format(utc(day))
  }
  return String(Number(day.slice(8)))
}

/** A time short enough to lead a title in a day cell: "9am", "2:30pm",
 *  or "14:30" where the locale counts to 24. */
export function formatCellTime(time: string, locale: string): string {
  const [h, m] = time.split(':').map(Number)
  const parts = formatter(locale, { hour: 'numeric', minute: '2-digit' }).formatToParts(
    new Date(Date.UTC(2000, 0, 1, h, m)),
  )
  const part = (type: string) => parts.find((p) => p.type === type)?.value ?? ''
  const period = part('dayPeriod')
  if (!period) return `${part('hour')}:${part('minute')}`
  const minutes = part('minute') === '00' ? '' : `:${part('minute')}`
  return `${part('hour')}${minutes}${period.toLowerCase().replace(/[\s.]/g, '')}`
}

// ---------------------------------------------------------------
// What sits on a day
// ---------------------------------------------------------------

/** The day a job sits on. Anchored on its dates, that is the plan day.
 *  Created and last activity are moments, placed on the day they fell
 *  on in the planning timezone. */
export function anchorDay(card: CardData, field: DateField, tz: string = planningTimezone()): Day | null {
  if (field === 'due_date') return planDay(card)
  const value = field === 'created_at' ? card.created_at : card.last_activity_at
  if (!value) return null
  const moment = new Date(value)
  return Number.isNaN(moment.getTime()) ? null : toCalendarDateString(moment, tz)
}

export interface DayJobs {
  /** Timed jobs in time order, then the rest in the day's order. */
  jobs: CardData[]
  /** How many of them lead with a time. */
  timed: number
}

const EMPTY: DayJobs = { jobs: [], timed: 0 }

/** Every job by the day it sits on, each day in reading order; jobs with
 *  no day under `null`. Anchored on a moment, no job leads with a time. */
export function jobsByDay(
  cards: readonly CardData[],
  field: DateField,
  tz: string = planningTimezone(),
): Map<Day | null, DayJobs> {
  const buckets = new Map<Day | null, CardData[]>()
  for (const card of cards) {
    const day = anchorDay(card, field, tz)
    const bucket = buckets.get(day)
    if (bucket) bucket.push(card)
    else buckets.set(day, [card])
  }
  const out = new Map<Day | null, DayJobs>()
  for (const [day, bucket] of buckets) {
    if (field !== 'due_date' || day == null) {
      out.set(day, { jobs: [...bucket].sort(compareAnyTime), timed: 0 })
      continue
    }
    const booked = bucket.filter((c) => planTime(c) != null).sort(compareBooked)
    const anyTime = bucket.filter((c) => planTime(c) == null).sort(compareAnyTime)
    out.set(day, { jobs: [...booked, ...anyTime], timed: booked.length })
  }
  return out
}

export function jobsOn(map: ReadonlyMap<Day | null, DayJobs>, day: Day | null): DayJobs {
  return map.get(day) ?? EMPTY
}

/** What a month cell that holds `lines` lines shows: as many jobs as fit,
 *  and when they don't all fit, the last line says how many more. */
export function cellLines<T>(jobs: readonly T[], lines: number): { shown: T[]; more: number } {
  const room = Math.max(1, lines)
  const shown = jobs.length > room ? jobs.slice(0, room - 1) : [...jobs]
  return { shown, more: jobs.length - shown.length }
}

// ---------------------------------------------------------------
// Done and late
// ---------------------------------------------------------------

/** Now on the planning timezone's wall clock: a day and `HH:MM`. */
export interface WallClock {
  day: Day
  time: string
}

export function wallClock(tz: string = planningTimezone(), now: Date = new Date()): WallClock {
  const parts = new Intl.DateTimeFormat('en-CA', {
    timeZone: tz,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
  }).formatToParts(now)
  const part = (type: string) => parts.find((p) => p.type === type)?.value ?? ''
  return { day: `${part('year')}-${part('month')}-${part('day')}`, time: `${part('hour')}:${part('minute')}` }
}

/** Finished or called off: nothing left to do. */
export function isClosed(card: CardData): boolean {
  return card.workflow_state.category === 'done' || card.workflow_state.category === 'cancelled'
}

/** Open and past its deadline: the due day has ended, or its due time
 *  has passed today. A job due today with no time is not late until
 *  tomorrow. */
export function isLate(card: CardData, clock: WallClock): boolean {
  if (isClosed(card)) return false
  const due = dueDay(card)
  if (!due) return false
  if (due !== clock.day) return due < clock.day
  return card.due_time != null && card.due_time.slice(0, 5) < clock.time
}

/** The first late job in reading order: the earliest day, then that
 *  day's order. Only days that pass `within` are read. */
export function earliestLate(
  byDay: ReadonlyMap<Day | null, DayJobs>,
  clock: WallClock,
  within: (day: Day) => boolean = () => true,
): { day: Day; card: CardData } | null {
  const days = [...byDay.keys()].filter((d): d is Day => d != null && within(d)).sort()
  for (const day of days) {
    const card = jobsOn(byDay, day).jobs.find((c) => isLate(c, clock))
    if (card) return { day, card }
  }
  return null
}

// ---------------------------------------------------------------
// The agenda's days
// ---------------------------------------------------------------

/** Every day the agenda can list: each day with a job or another date on
 *  it, and today whatever is on it, in order. */
export function agendaDays(jobDays: Iterable<Day | null>, otherDays: Iterable<Day>, today: Day): Day[] {
  const set = new Set<Day>([today])
  for (const day of jobDays) if (day != null) set.add(day)
  for (const day of otherDays) set.add(day)
  return [...set].sort()
}

export interface AgendaWindow {
  /** The days on screen, in order. */
  shown: Day[]
  /** Days before the first shown one are still to come in. */
  moreEarlier: boolean
  /** Days after the last shown one are still to come in. */
  moreAhead: boolean
}

/** The agenda's days on screen: `before` days before today and `after`
 *  from today on (today counts), each counted in listed days. */
export function agendaWindow(days: readonly Day[], today: Day, before: number, after: number): AgendaWindow {
  const earlier = days.filter((day) => day < today)
  const ahead = days.filter((day) => day >= today)
  const back = Math.max(0, before)
  return {
    shown: [...(back > 0 ? earlier.slice(-back) : []), ...ahead.slice(0, Math.max(0, after))],
    moreEarlier: earlier.length > back,
    moreAhead: ahead.length > after,
  }
}

/** How many days before and after today the window needs to list `day`,
 *  growing in steps of `step` from where it is. */
export function windowToReveal(
  days: readonly Day[],
  today: Day,
  day: Day,
  current: { before: number; after: number },
  step: number,
): { before: number; after: number } {
  const grow = (have: number, need: number) => (need <= have ? have : have + Math.ceil((need - have) / step) * step)
  if (day < today) {
    const earlier = days.filter((d) => d < today)
    const at = earlier.indexOf(day)
    if (at < 0) return current
    return { ...current, before: grow(current.before, earlier.length - at) }
  }
  const ahead = days.filter((d) => d >= today)
  const at = ahead.indexOf(day)
  if (at < 0) return current
  return { ...current, after: grow(current.after, at + 1) }
}

// ---------------------------------------------------------------
// Overlays to fetch
// ---------------------------------------------------------------

/** How far past today the agenda looks for dates that are not jobs, in
 *  months, when its jobs end sooner. */
export const AGENDA_HORIZON_MONTHS = 6

/** The window of days whose other dates (warranties and the like) the
 *  agenda lists: from the first of the month of its earliest job, or of
 *  today's, to the end of the month of its last job, or of the horizon. */
export function agendaRange(jobDays: Iterable<Day | null>, today: Day): { start: Day; end: Day } {
  let first = today
  let last = parseDate(today).add({ months: AGENDA_HORIZON_MONTHS }).toString()
  for (const day of jobDays) {
    if (day == null) continue
    if (day < first) first = day
    if (day > last) last = day
  }
  const end = parseDate(`${monthOf(last)}-01`).add({ months: 1 }).subtract({ days: 1 })
  return { start: `${monthOf(first)}-01`, end: end.toString() }
}

// ---------------------------------------------------------------
// Carrying a job to another day
// ---------------------------------------------------------------

/**
 * The write a job carried to `day` makes: the date that supplies its plan
 * day moves there (the start date when set, else the due date), and its
 * time stays as it is. Null when the job is already on that day. The
 * store adds what follows, as the server will (a move clears the job's
 * place in the old day's order).
 */
export function movePatch(
  card: CardData,
  day: Day,
): { start_date: string } | { due_date: string } | null {
  if (planDay(card) === day) return null
  const date = `${day}T00:00:00`
  return planField(card) === 'start' ? { start_date: date } : { due_date: date }
}
