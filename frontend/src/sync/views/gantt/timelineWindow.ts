/**
 * Vertical-timeline window: the date range the canvas covers.
 *
 * Pure so "cycles expand the canvas, tickets do not have to" is an
 * assertion rather than an eyeball of whatever demo data is loaded.
 */
import { addDays, differenceInCalendarDays, startOfDay } from 'date-fns'

export interface DateSpan {
  start: Date
  end: Date
}

/**
 * Window covering every provided span, padded a day each side, and
 * never shorter than a screenful of time so a short plan does not
 * leave a blank two-thirds of the display.
 *
 * `spans` is tickets and cycle extents; callers decide the union.
 * Empty input falls back to a fortnight around today.
 */
export function computeTimelineWindow(
  spans: readonly DateSpan[],
  opts: { viewportHeight: number; pxPerDay: number; now?: Date },
): { start: Date; days: number } {
  const today = startOfDay(opts.now ?? new Date())
  if (spans.length === 0) {
    return { start: addDays(today, -1), days: 14 }
  }
  let min = spans[0].start
  let max = spans[0].end
  for (const s of spans) {
    if (s.start < min) min = s.start
    if (s.end > max) max = s.end
  }
  const start = addDays(startOfDay(min), -1)
  const spanned = differenceInCalendarDays(max, start) + 2
  const screenful = Math.ceil(opts.viewportHeight / opts.pxPerDay)
  return { start, days: Math.max(3, spanned, screenful) }
}
