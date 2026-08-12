/**
 * Vertical-timeline window: the date range the canvas covers, and where
 * to open inside it.
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

/** Fraction of the viewport left above the anchor, so the thing you opened
 *  for sits just below the top edge with a little history for context. */
const LEAD = 0.25

/**
 * Where the canvas should be scrolled to on open.
 *
 * The window is bounded by cycles as well as tickets, so a project whose
 * cycles began months ago gets a canvas several screens tall with all the
 * live work at the bottom. Opening at the top then shows an empty calendar:
 * measured at 6 bars, 0 of them visible, the first one 2988px down. Cycle
 * bands expanding the window is deliberate, so the fix belongs here rather
 * than in the window.
 *
 * Anchors on today, then clamps that anchor into the range the work actually
 * occupies. One rule covers every case: today near the work opens on today,
 * an all-past plan opens on its last bar, an all-future plan on its first,
 * and a project with nothing scheduled opens on today.
 *
 * `todayY` is unclamped — it may sit outside the canvas, which is exactly how
 * an all-past or all-future plan is detected.
 */
export function landingScrollTop(opts: {
  todayY: number
  firstBarTop: number | null
  lastBarBottom: number | null
  viewportHeight: number
  canvasHeight: number
}): number {
  const { todayY, firstBarTop, lastBarBottom, viewportHeight, canvasHeight } = opts
  let anchor = todayY
  if (firstBarTop !== null && lastBarBottom !== null) {
    if (anchor < firstBarTop) anchor = firstBarTop
    else if (anchor > lastBarBottom) anchor = lastBarBottom
  }
  const maxScroll = Math.max(0, canvasHeight - viewportHeight)
  return Math.min(maxScroll, Math.max(0, Math.round(anchor - viewportHeight * LEAD)))
}
