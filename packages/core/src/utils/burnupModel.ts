/**
 * Pure date-math for the cycle burnup chart: working-day counting, the
 * adaptive pace reference, and the rate-based completion forecast.
 *
 * Grounded in the burnup research (see docs/plans/gantt-cycle-design-
 * overhaul.md): the naive straight "ideal" line is dropped in favour of
 * (1) an adaptive PACE line that distributes remaining scope over the
 * remaining working days (Linear's model, flat across weekends), and
 * (2) a FORECAST projected from the team's actual completion rate
 * (count-based throughput, after Vacanti / Cohn). Both are anchored at
 * today's real completed count, so neither reads as a day-zero guilt
 * line.
 *
 * All functions are timezone-stable: day keys are "YYYY-MM-DD" parsed at
 * UTC midnight to match the backend's day boundaries, and `now` / today
 * are injectable so callers stay testable.
 */

const DAY_MS = 86_400_000

/** Parse a "YYYY-MM-DD" key (or ISO datetime) to UTC-midnight epoch ms. */
export function parseDayMs(day: string): number {
  // Take the date portion only, so an rfc3339 datetime and a bare day
  // key land on the same UTC midnight.
  const datePart = day.slice(0, 10)
  return Date.parse(`${datePart}T00:00:00Z`)
}

/** UTC-midnight epoch ms for a given instant's calendar day. */
export function dayMsOf(ms: number): number {
  return Math.floor(ms / DAY_MS) * DAY_MS
}

/** Mon-Fri are working days; Sat/Sun are not. */
export function isWorkingDay(dayMs: number): boolean {
  const dow = new Date(dayMs).getUTCDay()
  return dow !== 0 && dow !== 6
}

/**
 * Count working days in the inclusive range [fromMs, toMs]. Returns 0
 * when the range is empty (from after to). Both ends snapped to day.
 */
export function countWorkingDays(fromMs: number, toMs: number): number {
  const from = dayMsOf(fromMs)
  const to = dayMsOf(toMs)
  if (to < from) return 0
  let count = 0
  for (let d = from; d <= to; d += DAY_MS) {
    if (isWorkingDay(d)) count++
  }
  return count
}

export interface SeriesPoint {
  day: string
  value: number
}

/**
 * Adaptive pace line from today to the cycle end: starts at today's
 * completed count and rises by an equal share of the remaining scope on
 * each remaining working day, staying flat across weekends. Reaches the
 * current scope on the final working day. Empty when there is no room to
 * pace (no remaining working days, or already at/over scope).
 */
export function buildPaceSeries(opts: {
  todayMs: number
  endMs: number
  completedToday: number
  scope: number
}): SeriesPoint[] {
  const today = dayMsOf(opts.todayMs)
  const end = dayMsOf(opts.endMs)
  const remaining = opts.scope - opts.completedToday
  if (end <= today || remaining <= 0) return []

  // Working days strictly after today, through the end day.
  const remainingWorking = countWorkingDays(today + DAY_MS, end)
  if (remainingWorking <= 0) return []
  const perWorkingDay = remaining / remainingWorking

  const out: SeriesPoint[] = [{ day: toDayKey(today), value: opts.completedToday }]
  let cumulative = opts.completedToday
  for (let d = today + DAY_MS; d <= end; d += DAY_MS) {
    if (isWorkingDay(d)) cumulative += perWorkingDay
    out.push({ day: toDayKey(d), value: cumulative })
  }
  return out
}

export interface ForecastResult {
  /** Projected completed-count line from today to the cycle end. */
  series: SeriesPoint[]
  /** Day key the projection first reaches scope, or null if not within window / no rate. */
  projectedFinishDay: string | null
  /** Items completed per working day, from actuals so far. */
  ratePerWorkingDay: number
}

/**
 * Rate-based forecast: extends today's completed count forward at the
 * team's observed throughput (completed so far / working days elapsed).
 * Flat across weekends, like the pace line, so the two are comparable.
 * The gap between this and the pace line at the end is the at-risk read.
 */
export function buildForecast(opts: {
  startMs: number
  todayMs: number
  endMs: number
  completedToday: number
  scope: number
}): ForecastResult {
  const start = dayMsOf(opts.startMs)
  const today = dayMsOf(opts.todayMs)
  const end = dayMsOf(opts.endMs)

  const elapsedWorking = Math.max(1, countWorkingDays(start, today))
  const rate = opts.completedToday / elapsedWorking

  if (end <= today || rate <= 0) {
    return { series: [], projectedFinishDay: null, ratePerWorkingDay: rate }
  }

  const out: SeriesPoint[] = [{ day: toDayKey(today), value: opts.completedToday }]
  let cumulative = opts.completedToday
  let projectedFinishDay: string | null = null
  for (let d = today + DAY_MS; d <= end; d += DAY_MS) {
    if (isWorkingDay(d)) cumulative += rate
    out.push({ day: toDayKey(d), value: cumulative })
    if (projectedFinishDay === null && cumulative >= opts.scope) {
      projectedFinishDay = toDayKey(d)
    }
  }
  return { series: out, projectedFinishDay, ratePerWorkingDay: rate }
}

function toDayKey(dayMs: number): string {
  return new Date(dayMs).toISOString().slice(0, 10)
}
