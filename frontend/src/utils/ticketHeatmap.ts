/**
 * Calendar days for the 365-day ticket heatmap, reckoned in the viewer's
 * zone (`dateStore.effectiveTimezone`, the zone dates render in) so a
 * ticket closed at 09:30 in Sydney lands on that Sydney day, not on the
 * UTC date. Day keys are `YYYY-MM-DD`; every helper takes the zone
 * explicitly so specs can pin one.
 */
import type { RouteLocationRaw } from 'vue-router'
import {
  calendarDayOf,
  fromCalendarDateString,
  toCalendarDateString,
} from '@nosdesk/core/utils/dateMath'
import { parseDate } from '@nosdesk/core/utils/dateUtils'
import { buildDashboardTicketDrillDown } from '@/utils/dashboardTicketDrillDown'

export type TicketHeatmapMode = 'completed' | 'active'

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

/**
 * The day a ticket counts on. Completed mode reads `closed_at`, falling
 * back to `modified` for tickets closed before the database stamped
 * `closed_at`; active mode reads `modified`.
 */
export function ticketHeatmapDay(
  ticket: { closed_at?: string | null; modified: string },
  mode: TicketHeatmapMode,
  tz: string,
): string | null {
  const stamp = mode === 'completed' && ticket.closed_at ? ticket.closed_at : ticket.modified
  const date = parseDate(stamp)
  return date ? toCalendarDateString(date, tz) : null
}

/** Local midnight of `key` to local midnight of the next day, in `tz`. */
export function heatmapDayWindow(key: string, tz: string): { from: string; to: string } {
  const from = fromCalendarDateString(key, tz)
  const to = fromCalendarDateString(shiftDayKey(key, 1), tz)
  return { from: from?.toISOString() ?? '', to: to?.toISOString() ?? '' }
}

/** The ticket list for one heatmap day: closed that day, or created that day. */
export function heatmapDayRoute(key: string, mode: TicketHeatmapMode, tz: string): RouteLocationRaw {
  return buildDashboardTicketDrillDown(
    mode === 'completed' ? 'dashboard-resolved' : 'dashboard-created',
    heatmapDayWindow(key, tz),
  )
}
