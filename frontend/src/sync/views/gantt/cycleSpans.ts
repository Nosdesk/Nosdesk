/**
 * Cycle spans in date-space. Pure: no pixels, no Vue.
 *
 * Desktop maps with `xOf`; the vertical timeline maps with
 * `days * pxPerDay`. Both read the same half-open span so inclusive
 * `end_at` days never drift between the two renderers.
 */
import { addDays } from 'date-fns'
import { startOfDay } from '@/composables/useGanttViewport'
import type { GanttCycle } from './types'

export interface CycleSpan {
  key: string
  start: Date
  /** Exclusive end (day after the inclusive end_at). */
  endExclusive: Date
  label: string
  state: GanttCycle['state']
}

/**
 * Cycles with both dates become spans covering their inclusive range.
 * Undated cycles are context for grouping, not canvas geometry.
 */
export function datedCycleSpans(cycles: readonly GanttCycle[]): CycleSpan[] {
  const out: CycleSpan[] = []
  for (const c of cycles) {
    if (!c.start_at || !c.end_at) continue
    const start = startOfDay(new Date(c.start_at))
    const endExclusive = addDays(startOfDay(new Date(c.end_at)), 1)
    if (endExclusive.getTime() <= start.getTime()) continue
    out.push({
      key: c.uuid,
      start,
      endExclusive,
      label: c.name,
      state: c.state,
    })
  }
  return out
}

/** Strip-label tint by cycle state: active stands out, planned is
 *  neutral, completed is muted. */
export function cycleStripClass(state: GanttCycle['state']): string {
  if (state === 'active') return 'bg-accent/15 text-accent border-accent/40'
  if (state === 'planned') return 'bg-surface-hover text-secondary border-subtle'
  return 'bg-surface-alt text-tertiary border-subtle'
}

/** Body shading: a faint wash so the band reads behind the bars
 *  without competing with them. */
export function cycleBodyClass(state: GanttCycle['state']): string {
  return state === 'active' ? 'bg-accent/5' : 'bg-surface-hover/30'
}

export interface ProjectedBand {
  key: string
  /** Offset along the time axis (px). */
  offset: number
  /** Extent along the time axis (px). */
  extent: number
  label: string
  state: GanttCycle['state']
}

/**
 * Project a date-space span onto a 1D canvas of `pxPerDay`, clipped
 * to `[0, canvasExtent]`. Returns null when the span is entirely
 * outside the canvas (same skip rule desktop uses for off-canvas cycles).
 */
export function projectCycleBand(
  span: CycleSpan,
  canvasStart: Date,
  canvasExtentPx: number,
  pxPerDay: number,
  dayOffset: (from: Date, to: Date) => number,
): ProjectedBand | null {
  const rawStart = dayOffset(canvasStart, span.start) * pxPerDay
  const rawEnd = dayOffset(canvasStart, span.endExclusive) * pxPerDay
  if (rawEnd <= 0 || rawStart >= canvasExtentPx) return null
  const offset = Math.max(0, rawStart)
  return {
    key: span.key,
    offset,
    extent: Math.max(1, Math.min(canvasExtentPx, rawEnd) - offset),
    label: span.label,
    state: span.state,
  }
}
