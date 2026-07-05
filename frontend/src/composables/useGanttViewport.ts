/**
 * Shared Gantt viewport: a horizontally scrollable timeline canvas.
 *
 * The canvas is a fixed date range (project content padded by a few
 * months, unioned with today's neighbourhood) rendered at
 * `pxPerDay` for the current zoom. Navigation is native scrolling;
 * Today / Fit / pan are smooth `scrollTo` calls, and zoom keeps the
 * date under the anchor fixed. This replaces the earlier
 * fixed-window model (width-fill + pan buttons), matching how
 * Linear / Asana / Jira timelines navigate.
 *
 * Owned by the route shell (which renders the toolbar in the
 * project tab bar) and consumed by the renderer (GanttBoard), which
 * reads the same refs for every geometry calc. `xOf(date)` is the
 * single projection from a date to a canvas pixel offset; bars,
 * axis ticks, the today line, and dependency arrows all read
 * through it.
 *
 * The canvas origin (`canvasStart`) only ever moves during a
 * session when new content requires it, and any leftward extension
 * compensates the scroll position in the same DOM patch so nothing
 * visibly jumps.
 *
 * Viewport state persists per project (zoom + left-edge date) so
 * the board reopens where the user left it.
 */
import { computed, nextTick, ref, toValue, watch, type ComputedRef, type MaybeRefOrGetter, type Ref } from 'vue'
import { addDays, addMonths, startOfMonth } from 'date-fns'
import { logger } from '@nosdesk/core/utils/logger'
import { scrollBehavior } from '@/composables/useReducedMotion'

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

/** Content padding on each side of the project span (months). */
const CONTENT_PAD_MONTHS = 3
/** Today's neighbourhood always included in the canvas (months). */
const TODAY_PAD_MONTHS = 6
/** Left inset when framing a project (days at month zoom scale). */
function fitPadDays(pxPerDay: number): number {
  return Math.max(3, Math.round(7 / (pxPerDay / 9)))
}

interface PersistedViewport {
  zoom: GanttZoom
  /** ISO date of the day at the viewport's left edge. Stored as a
   * date (not px) so it survives zoom changes and canvas
   * re-derivation. */
  leftDate: string
}

export interface GanttViewport {
  zoom: Ref<GanttZoom>
  pxPerDay: Ref<number>
  /** Full rendered canvas range. Grow-only within a project. */
  canvasStart: Ref<Date>
  canvasEnd: Ref<Date>
  totalWidth: Ref<number>
  xOf: (date: Date) => number
  dateAt: (px: number) => Date
  /** Live scroll offset (px) of the timeline, rAF-coalesced. */
  scrollX: Ref<number>
  /** Visible timeline width (px), excluding the lane column. */
  viewportWidth: Ref<number>
  /** The date window currently on screen. Decoration layers window
   * their work to this (plus a buffer); bars render regardless. */
  visibleRange: ComputedRef<{ start: Date; end: Date }>
  /** Wire the scroll container + lane element. The viewport owns
   * scroll listening, size observation, and the initial restore /
   * fit once everything is measured. */
  attachScroller: (el: HTMLElement | null, laneEl?: HTMLElement | null) => void
  /** Content extent reported by the renderer; drives the canvas
   * range and Fit. */
  contentBounds: Ref<{ min: Date; max: Date } | null>
  setContentBounds: (b: { min: Date; max: Date } | null) => void
  /** Bars currently intersecting the visible window, reported by
   * the renderer for the toolbar's in-view label. */
  visibleCount: Ref<number>
  fitToProject: () => void
  /** Switch zoom keeping the date under `anchorPx` (px from the
   * visible timeline's left edge; defaults to center) fixed. */
  setZoom: (z: GanttZoom, anchorPx?: number) => void
  centerOnToday: () => void
  pan: (dir: -1 | 1) => void
}

export function useGanttViewport(
  options: { storageKey?: MaybeRefOrGetter<string | null> } = {},
): GanttViewport {
  const zoom = ref<GanttZoom>('month')
  const pxPerDay = computed(() => PX_PER_DAY[zoom.value])

  // ---------------- canvas range ----------------

  const contentBounds = ref<{ min: Date; max: Date } | null>(null)
  const canvasStart = ref<Date>(defaultCanvasStart())
  const canvasEnd = ref<Date>(defaultCanvasEnd())

  function defaultCanvasStart(): Date {
    return startOfMonth(addMonths(startOfDay(new Date()), -TODAY_PAD_MONTHS))
  }
  function defaultCanvasEnd(): Date {
    return startOfMonth(addMonths(startOfDay(new Date()), TODAY_PAD_MONTHS + 1))
  }

  /** Extend the canvas to cover the content bounds (grow-only; a
   * leftward extension compensates scrollLeft in the same patch so
   * the view doesn't jump). */
  function growCanvas(): void {
    const b = contentBounds.value
    if (!b) return
    const wantStart = startOfMonth(addMonths(b.min, -CONTENT_PAD_MONTHS))
    const wantEnd = startOfMonth(addMonths(b.max, CONTENT_PAD_MONTHS + 1))
    if (wantEnd.getTime() > canvasEnd.value.getTime()) {
      canvasEnd.value = wantEnd
    }
    if (wantStart.getTime() < canvasStart.value.getTime()) {
      const deltaDays = daysBetween(wantStart, canvasStart.value)
      canvasStart.value = wantStart
      // Every canvas coordinate just moved right by deltaDays; shift
      // the scroll position with the content after the DOM patch
      // (nextTick runs post-update, pre-paint, so this is atomic).
      const el = scroller.value
      if (el) {
        void nextTick(() => {
          el.scrollLeft += deltaDays * pxPerDay.value
          syncScrollX()
        })
      }
    }
  }

  function setContentBounds(b: { min: Date; max: Date } | null): void {
    contentBounds.value = b
    growCanvas()
    maybeInitialise()
  }

  const canvasDays = computed(() => Math.max(1, daysBetween(canvasStart.value, canvasEnd.value)))
  const totalWidth = computed(() => Math.round(canvasDays.value * pxPerDay.value))

  function xOf(date: Date): number {
    return daysBetween(canvasStart.value, date) * pxPerDay.value
  }
  function dateAt(px: number): Date {
    return addDays(canvasStart.value, Math.round(px / pxPerDay.value))
  }

  // ---------------- scroller wiring ----------------

  const scroller = ref<HTMLElement | null>(null)
  const laneEl = ref<HTMLElement | null>(null)
  const scrollX = ref(0)
  const viewportWidth = ref(0)

  let rafPending = false
  function onScroll(): void {
    if (rafPending) return
    rafPending = true
    requestAnimationFrame(() => {
      rafPending = false
      syncScrollX()
    })
  }
  function syncScrollX(): void {
    const el = scroller.value
    if (el) scrollX.value = el.scrollLeft
  }

  let resizeObserver: ResizeObserver | null = null
  function measure(): void {
    const el = scroller.value
    if (!el) return
    const lane = laneEl.value?.offsetWidth ?? 0
    viewportWidth.value = Math.max(0, el.clientWidth - lane)
  }

  function attachScroller(el: HTMLElement | null, lane?: HTMLElement | null): void {
    if (scroller.value === el && laneEl.value === (lane ?? null)) return
    scroller.value?.removeEventListener('scroll', onScroll)
    resizeObserver?.disconnect()
    resizeObserver = null
    scroller.value = el
    laneEl.value = lane ?? null
    if (!el) return
    el.addEventListener('scroll', onScroll, { passive: true })
    resizeObserver = new ResizeObserver(measure)
    resizeObserver.observe(el)
    if (laneEl.value) resizeObserver.observe(laneEl.value)
    measure()
    syncScrollX()
    maybeInitialise()
  }

  const visibleRange = computed(() => ({
    start: dateAt(scrollX.value),
    end: dateAt(scrollX.value + Math.max(viewportWidth.value, 1)),
  }))

  const visibleCount = ref(0)

  // ---------------- operations ----------------

  function scrollToX(x: number, smooth = true): void {
    const el = scroller.value
    if (!el) return
    el.scrollTo({ left: Math.max(0, x), behavior: smooth ? scrollBehavior() : 'auto' })
  }

  /** Densest zoom whose padded content span fits the viewport. */
  function zoomThatFits(spanDays: number): GanttZoom {
    const vw = viewportWidth.value
    if (vw <= 0) return zoom.value
    for (const z of GANTT_ZOOMS) {
      if (spanDays * PX_PER_DAY[z] <= vw) return z
    }
    return 'quarter'
  }

  function fitToProject(smooth = true): void {
    const b = contentBounds.value
    if (!b) {
      centerOnToday()
      return
    }
    const pad = fitPadDays(pxPerDay.value)
    const spanDays = daysBetween(b.min, b.max) + pad * 2
    const target = zoomThatFits(spanDays)
    const apply = (): void => scrollToX(xOf(addDays(b.min, -fitPadDays(pxPerDay.value))), smooth)
    if (target !== zoom.value) {
      zoom.value = target
      // Canvas width changes with pxPerDay; scroll after the patch.
      void nextTick(apply)
    } else {
      apply()
    }
  }

  function setZoom(z: GanttZoom, anchorPx?: number): void {
    if (z === zoom.value) return
    const el = scroller.value
    const anchor = anchorPx ?? viewportWidth.value / 2
    const anchorDate = dateAt(scrollX.value + anchor)
    zoom.value = z
    if (!el) return
    void nextTick(() => {
      el.scrollLeft = Math.max(0, xOf(anchorDate) - anchor)
      syncScrollX()
    })
  }

  function centerOnToday(smooth = true): void {
    scrollToX(xOf(startOfDay(new Date())) - viewportWidth.value / 2, smooth)
  }

  function pan(dir: -1 | 1): void {
    const el = scroller.value
    if (!el) return
    el.scrollBy({ left: dir * viewportWidth.value * 0.8, behavior: scrollBehavior() })
  }

  // ---------------- persistence + initial framing ----------------

  const storageKey = computed(() => toValue(options.storageKey) ?? null)
  let initialised = false

  function load(): PersistedViewport | null {
    const key = storageKey.value
    if (!key) return null
    try {
      const raw = localStorage.getItem(key)
      if (!raw) return null
      const parsed = JSON.parse(raw) as PersistedViewport
      if (!GANTT_ZOOMS.includes(parsed.zoom)) return null
      if (Number.isNaN(new Date(parsed.leftDate).getTime())) return null
      return parsed
    } catch {
      return null
    }
  }

  let saveTimer: ReturnType<typeof setTimeout> | null = null
  function scheduleSave(): void {
    if (!initialised || !storageKey.value) return
    if (saveTimer) clearTimeout(saveTimer)
    saveTimer = setTimeout(() => {
      saveTimer = null
      const key = storageKey.value
      if (!key || !initialised) return
      try {
        const state: PersistedViewport = {
          zoom: zoom.value,
          leftDate: dateAt(scrollX.value).toISOString(),
        }
        localStorage.setItem(key, JSON.stringify(state))
      } catch (e) {
        logger.debug('gantt viewport persist failed', { error: e })
      }
    }, 300)
  }
  watch([scrollX, zoom], scheduleSave)

  /** First framing, once the scroller is attached and measured:
   * restore the persisted viewport if any, else fit the project.
   * Waits for content bounds when a fit will be needed. */
  function maybeInitialise(): void {
    if (initialised) return
    if (!scroller.value || viewportWidth.value <= 0) return
    const stored = load()
    if (stored) {
      initialised = true
      zoom.value = stored.zoom
      const left = startOfDay(new Date(stored.leftDate))
      // Clamp into the canvas: a stored position outside the
      // re-derived range lands at the nearest edge.
      void nextTick(() => {
        scrollToX(xOf(left), false)
        syncScrollX()
      })
      return
    }
    if (!contentBounds.value) return
    initialised = true
    void nextTick(() => fitToProject(false))
  }

  // Project switch: re-frame for the new key. Canvas resets so one
  // project's sprawl doesn't bleed into the next.
  watch(storageKey, () => {
    initialised = false
    contentBounds.value = null
    canvasStart.value = defaultCanvasStart()
    canvasEnd.value = defaultCanvasEnd()
    maybeInitialise()
  })

  watch(viewportWidth, () => maybeInitialise())

  return {
    zoom,
    pxPerDay,
    canvasStart,
    canvasEnd,
    totalWidth,
    xOf,
    dateAt,
    scrollX,
    viewportWidth,
    visibleRange,
    attachScroller,
    contentBounds,
    setContentBounds,
    visibleCount,
    fitToProject: () => fitToProject(true),
    setZoom,
    centerOnToday: () => centerOnToday(true),
    pan,
  }
}
