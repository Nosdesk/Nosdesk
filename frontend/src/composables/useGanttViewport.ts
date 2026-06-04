/**
 * Shared Gantt viewport: the time-scale state (zoom + visible date
 * window) plus the pan / fit / today / zoom operations on it.
 *
 * Lives in a composable so the viewport can be OWNED by the route
 * shell (which renders the toolbar in the project tab bar) and
 * CONSUMED by the renderer (GanttBoard), which reads the same refs
 * for every geometry calc. The renderer reports its content extent
 * and visible-bar count back through here so Fit and the in-view
 * label work without the toolbar reaching into the renderer.
 *
 * `xOf(date)` is the single projection from a date to a pixel offset;
 * bars, axis ticks, the today line, and dependency arrows all read
 * through it, so the board is a pure function of
 * (cards, edges, [rangeStart, rangeEnd], pxPerDay).
 */
import { computed, ref, type Ref } from 'vue'
import { addDays } from 'date-fns'

export type GanttZoom = 'week' | 'month' | 'quarter'

const PX_PER_DAY: Record<GanttZoom, number> = { week: 26, month: 9, quarter: 3.4 }
const DAY_MS = 86_400_000

export const GANTT_ZOOMS: GanttZoom[] = ['week', 'month', 'quarter']
export const ganttZoomLabel: Record<GanttZoom, string> = {
  week: 'gantt-zoom-week',
  month: 'gantt-zoom-month',
  quarter: 'gantt-zoom-quarter',
}

/** Truncate to local midnight without mutating the input. */
export function startOfDay(d: Date): Date {
  const x = new Date(d)
  x.setHours(0, 0, 0, 0)
  return x
}

export function daysBetween(a: Date, b: Date): number {
  return Math.round((startOfDay(b).getTime() - startOfDay(a).getTime()) / DAY_MS)
}

export interface GanttViewport {
  zoom: Ref<GanttZoom>
  pxPerDay: Ref<number>
  rangeStart: Ref<Date>
  rangeEnd: Ref<Date>
  xOf: (date: Date) => number
  totalWidth: Ref<number>
  /** Content extent reported by the renderer, used to frame Fit. */
  contentBounds: Ref<{ min: Date; max: Date } | null>
  setContentBounds: (b: { min: Date; max: Date } | null) => void
  /** Bars currently inside the window, reported for the in-view label. */
  visibleCount: Ref<number>
  fitToProject: () => void
  setZoom: (z: GanttZoom) => void
  centerOnToday: () => void
  pan: (dir: -1 | 1) => void
}

export function useGanttViewport(): GanttViewport {
  const zoom = ref<GanttZoom>('month')
  const pxPerDay = computed(() => PX_PER_DAY[zoom.value])

  const rangeStart = ref<Date>(startOfDay(new Date()))
  const rangeEnd = ref<Date>(addDays(startOfDay(new Date()), 45))

  function xOf(date: Date): number {
    return daysBetween(rangeStart.value, date) * pxPerDay.value
  }
  const totalWidth = computed(() => xOf(rangeEnd.value))

  const contentBounds = ref<{ min: Date; max: Date } | null>(null)
  function setContentBounds(b: { min: Date; max: Date } | null): void {
    contentBounds.value = b
  }

  const visibleCount = ref(0)

  /** Padding around the project span, in days. Wider when zoomed out
   *  so the canvas never crowds the edges. */
  function fitPad(): number {
    return Math.max(3, Math.round(7 / (pxPerDay.value / 9)))
  }

  function fitToProject(): void {
    const b = contentBounds.value
    if (!b) {
      const today = startOfDay(new Date())
      rangeStart.value = addDays(today, -7)
      rangeEnd.value = addDays(today, 45)
      return
    }
    const pad = fitPad()
    rangeStart.value = addDays(b.min, -pad)
    rangeEnd.value = addDays(b.max, pad)
  }

  function setZoom(z: GanttZoom): void {
    if (z === zoom.value) return
    // Keep the same center across a zoom change: re-derive the window
    // around its current midpoint so the user's focus stays put rather
    // than snapping back to the whole project.
    const span = daysBetween(rangeStart.value, rangeEnd.value)
    const center = addDays(rangeStart.value, Math.round(span / 2))
    zoom.value = z
    // Hold the on-screen span constant in days; the new pxPerDay just
    // changes how wide that span paints.
    const half = Math.round(span / 2)
    rangeStart.value = addDays(center, -half)
    rangeEnd.value = addDays(center, span - half)
  }

  function centerOnToday(): void {
    const span = daysBetween(rangeStart.value, rangeEnd.value)
    const today = startOfDay(new Date())
    const half = Math.round(span / 2)
    rangeStart.value = addDays(today, -half)
    rangeEnd.value = addDays(today, span - half)
  }

  function pan(dir: -1 | 1): void {
    const span = daysBetween(rangeStart.value, rangeEnd.value)
    const step = Math.max(1, Math.round(span * 0.4)) * dir
    rangeStart.value = addDays(rangeStart.value, step)
    rangeEnd.value = addDays(rangeEnd.value, step)
  }

  return {
    zoom,
    pxPerDay,
    rangeStart,
    rangeEnd,
    xOf,
    totalWidth,
    contentBounds,
    setContentBounds,
    visibleCount,
    fitToProject,
    setZoom,
    centerOnToday,
    pan,
  }
}
