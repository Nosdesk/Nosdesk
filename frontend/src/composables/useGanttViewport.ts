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
  /** Available timeline width in px (the scroll container minus the
   *  left title panel), reported by the renderer so the visible window
   *  always fills the panel instead of leaving dead space. */
  setViewportWidth: (px: number) => void
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

  // Available timeline width (px). 0 until the renderer measures it; a
  // sensible default span paints the first frame before measurement.
  const viewportWidth = ref(0)
  const DEFAULT_SPAN_DAYS = 45
  function setViewportWidth(px: number): void {
    viewportWidth.value = Math.max(0, Math.round(px))
  }

  // Days that exactly fill the measured width at the current zoom, so
  // the timeline never leaves dead space and shows more range on wider
  // displays. rangeEnd is derived from it; rangeStart is the only
  // positional state the operations move.
  const spanDays = computed(() =>
    viewportWidth.value > 0
      ? Math.max(1, Math.ceil(viewportWidth.value / pxPerDay.value))
      : DEFAULT_SPAN_DAYS,
  )

  const rangeStart = ref<Date>(addDays(startOfDay(new Date()), -7))
  const rangeEnd = computed(() => addDays(rangeStart.value, spanDays.value))

  function xOf(date: Date): number {
    return daysBetween(rangeStart.value, date) * pxPerDay.value
  }
  const totalWidth = computed(() => spanDays.value * pxPerDay.value)

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
      rangeStart.value = addDays(startOfDay(new Date()), -7)
      return
    }
    // Anchor the content near the left with padding; the span fills the
    // width from there. (Auto-zoom to frame a project wider than the
    // window is a follow-up; the zoom buttons cover density today.)
    rangeStart.value = addDays(b.min, -fitPad())
  }

  function setZoom(z: GanttZoom): void {
    if (z === zoom.value) return
    // Keep the same center across the zoom change. The span recomputes
    // for the new pxPerDay (a denser zoom fills the width with fewer
    // days), so re-anchor rangeStart around the held center.
    const center = addDays(rangeStart.value, Math.round(spanDays.value / 2))
    zoom.value = z
    rangeStart.value = addDays(center, -Math.round(spanDays.value / 2))
  }

  function centerOnToday(): void {
    const today = startOfDay(new Date())
    rangeStart.value = addDays(today, -Math.round(spanDays.value / 2))
  }

  function pan(dir: -1 | 1): void {
    const step = Math.max(1, Math.round(spanDays.value * 0.4)) * dir
    rangeStart.value = addDays(rangeStart.value, step)
  }

  return {
    zoom,
    pxPerDay,
    rangeStart,
    rangeEnd,
    xOf,
    totalWidth,
    setViewportWidth,
    contentBounds,
    setContentBounds,
    visibleCount,
    fitToProject,
    setZoom,
    centerOnToday,
    pan,
  }
}
