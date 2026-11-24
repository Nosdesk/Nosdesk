/**
 * The Completed work heatmap: a year of Done tickets, each on the day it
 * was closed, in the viewer's zone (`dateStore.effectiveTimezone`, the
 * zone dates render in), so a ticket closed at 09:30 in Sydney lands on
 * that Sydney day, not on the UTC date. Day keys are `YYYY-MM-DD`; every
 * helper takes the zone explicitly so specs can pin one.
 */
import type { RouteLocationRaw } from 'vue-router'
import {
  calendarDayOf,
  fromCalendarDateString,
  toCalendarDateString,
} from '@nosdesk/core/utils/dateMath'
import { formatDate, parseDate } from '@nosdesk/core/utils/dateUtils'
import { sizeOf, type Size } from '@nosdesk/core/utils/ticketPlan'
import { buildDashboardTicketDrillDown } from '@/utils/dashboardTicketDrillDown'

/** Whose completions the heatmap shows. */
export type HeatmapScope = 'mine' | 'everyone'

/** The pool row fields the heatmap reads. */
export interface HeatmapTicket {
  id: number
  title: string
  workflow_state: { category: string } | null
  assignee_uuid: string | null
  closed_at?: string | null
  closed_by?: string | null
  /** Planned length in minutes; null for an unsized job. */
  estimate_minutes?: number | null
}

export interface HeatmapDay {
  date: string
  /** Jobs done that day. */
  count: number
  /** Sum of the jobs' weights; the shade reads this. */
  weight: number
  /** Shade, 0 (nothing done) to 4 (a big day for this person). */
  level: number
  tickets: { id: number; title: string }[]
}

/** The day key `days` calendar days after `key` (negative goes back). */
export function shiftDayKey(key: string, days: number): string {
  const d = fromCalendarDateString(key, 'UTC')
  if (!d) return key
  d.setUTCDate(d.getUTCDate() + days)
  return d.toISOString().slice(0, 10)
}

/** Day of the week of a day key, 0 = Sunday. */
export function weekdayOfDayKey(key: string): number {
  return fromCalendarDateString(key, 'UTC')?.getUTCDay() ?? 0
}

/** The `count` day keys ending with today in `tz`, oldest first. */
export function heatmapDayKeys(now: Date, tz: string, count = 365): string[] {
  const today = calendarDayOf(now, tz)
  const keys: string[] = []
  for (let i = count - 1; i >= 0; i--) {
    keys.push(today.subtract({ days: i }).toString())
  }
  return keys
}

/** A completion is the assignee's, or whoever closed it when nobody was. */
export function creditedTo(ticket: HeatmapTicket): string | null {
  return ticket.assignee_uuid ?? ticket.closed_by ?? null
}

/**
 * The day a ticket counts as completed work, or null when it doesn't
 * count: only Done tickets (cancelled and merged aren't work done), on
 * the day of their `closed_at`, with no fallback when that is missing.
 */
export function completedDay(ticket: HeatmapTicket, tz: string): string | null {
  if (ticket.workflow_state?.category !== 'done' || !ticket.closed_at) return null
  const date = parseDate(ticket.closed_at)
  return date ? toCalendarDateString(date, tz) : null
}

/** Weight per size: small 1, medium 2, big 4. */
export const SIZE_WEIGHTS = { small: 1, medium: 2, big: 4 } as const

const WEIGHT_OF_SIZE: Record<Size, number> = {
  s: SIZE_WEIGHTS.small,
  m: SIZE_WEIGHTS.medium,
  l: SIZE_WEIGHTS.big,
}

/** A job's weight from its size (`sizeOf`: up to an hour is small, up to
 *  four hours medium, more is big); an unsized job weighs as medium. */
export function jobWeight(estimateMinutes?: number | null): number {
  const size = sizeOf(estimateMinutes)
  return size ? WEIGHT_OF_SIZE[size] : SIZE_WEIGHTS.medium
}

/** Boundaries used until there are enough active days for quartiles. */
export const FIXED_SHADE_BOUNDARIES: readonly [number, number, number] = [2, 4, 8]
const MIN_DAYS_FOR_QUARTILES = 8

function quantile(sorted: number[], q: number): number {
  const pos = (sorted.length - 1) * q
  const lo = Math.floor(pos)
  const hi = Math.ceil(pos)
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo)
}

/**
 * The three shade boundaries for a year of day weights: the quartiles of
 * the non-zero days, so the darkest shade means a big day for this
 * person, or 2, 4 and 8 with fewer than eight active days.
 */
export function shadeBoundaries(weights: number[]): readonly [number, number, number] {
  const active = weights.filter((w) => w > 0).sort((a, b) => a - b)
  if (active.length < MIN_DAYS_FOR_QUARTILES) return FIXED_SHADE_BOUNDARIES
  return [quantile(active, 0.25), quantile(active, 0.5), quantile(active, 0.75)]
}

/** Shade 0 to 4: 0 for nothing done, then one step per boundary the day
 *  exceeds, so a day at or under the first boundary is the lightest. */
export function shadeLevel(weight: number, boundaries: readonly [number, number, number]): number {
  if (weight <= 0) return 0
  return 1 + boundaries.filter((b) => weight > b).length
}

/** The year of completed work for `scope`, oldest day first. */
export function buildCompletedWork(
  tickets: Iterable<HeatmapTicket>,
  options: { now: Date; tz: string; scope: HeatmapScope; me: string | null },
): HeatmapDay[] {
  const days = heatmapDayKeys(options.now, options.tz).map<HeatmapDay>((date) => ({
    date,
    count: 0,
    weight: 0,
    level: 0,
    tickets: [],
  }))
  const byDate = new Map(days.map((d) => [d.date, d]))
  for (const ticket of tickets) {
    if (options.scope === 'mine' && (!options.me || creditedTo(ticket) !== options.me)) continue
    const key = completedDay(ticket, options.tz)
    const day = key ? byDate.get(key) : undefined
    if (!day) continue
    day.count++
    day.weight += jobWeight(ticket.estimate_minutes)
    day.tickets.push({ id: ticket.id, title: ticket.title })
  }
  const boundaries = shadeBoundaries(days.map((d) => d.weight))
  for (const day of days) day.level = shadeLevel(day.weight, boundaries)
  return days
}

/** Where this device remembers the Mine / Everyone choice: one entry per
 *  workspace and user, so a shared browser keeps each person's own. */
export function heatmapScopeStorageKey(workspaceId: number | null, userUuid: string | null): string | null {
  return workspaceId != null && userUuid ? `ticket-heatmap-scope:${workspaceId}:${userUuid}` : null
}

/** The stored choice, or null when there is none (or storage is out of
 *  reach), so the default rule applies. */
export function loadHeatmapScope(storage: Storage | undefined, key: string | null): HeatmapScope | null {
  if (!storage || !key) return null
  try {
    const value = storage.getItem(key)
    return value === 'mine' || value === 'everyone' ? value : null
  } catch {
    return null
  }
}

export function saveHeatmapScope(storage: Storage | undefined, key: string | null, scope: HeatmapScope): void {
  if (!storage || !key) return
  try {
    storage.setItem(key, scope)
  } catch {
    // A full or blocked storage only costs remembering the choice.
  }
}

/** Whether `me` has any ticket assigned, which makes Mine the default. */
export function hasAssignedWork(tickets: Iterable<HeatmapTicket>, me: string | null): boolean {
  if (!me) return false
  for (const ticket of tickets) if (ticket.assignee_uuid === me) return true
  return false
}

/** A day's hover line, "Tue 17 Nov: 5 jobs done", in the active locale.
 *  The day key is read and formatted in UTC so it names its own day in
 *  every zone. */
export function heatmapDayTitle(
  day: { date: string; count: number },
  t: (key: string, args: Record<string, string | number>) => string,
): string {
  return t('ticket-heatmap-day-done', {
    date: formatDate(day.date, 'weekdayMonthDay', 'UTC'),
    count: day.count,
  })
}

/** Local midnight of `key` to local midnight of the next day, in `tz`. */
export function heatmapDayWindow(key: string, tz: string): { from: string; to: string } {
  const from = fromCalendarDateString(key, tz)
  const to = fromCalendarDateString(shiftDayKey(key, 1), tz)
  return { from: from?.toISOString() ?? '', to: to?.toISOString() ?? '' }
}

/** The ticket list a day opens: the same jobs the cell counted. */
export function heatmapDayRoute(
  key: string,
  tz: string,
  scope: HeatmapScope,
  me: string | null,
): RouteLocationRaw {
  return buildDashboardTicketDrillDown('dashboard-resolved', heatmapDayWindow(key, tz), {
    doneOnly: true,
    creditedTo: scope === 'mine' && me ? me : undefined,
  })
}
