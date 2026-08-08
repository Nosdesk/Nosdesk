<script setup lang="ts">
/**
 * Zoom-aware Gantt renderer on a horizontally scrollable canvas.
 *
 * One scroll container owns both axes: the lane column is sticky
 * left, the axis header sticky top, and the corner cell sticky on
 * both. The timeline column is the full canvas width (the viewport
 * derives the range from the project's content bounds), so
 * navigation is native trackpad/wheel scrolling; the toolbar's
 * Today / Fit / pan are smooth scrolls over the same canvas.
 *
 * `xOf(date)` is the single projection from a date to a canvas
 * pixel offset; bars, axis ticks, the today line, and dependency
 * arrows all read through it. Bars are always rendered at their
 * true coordinates (rows never shuffle with scroll); only the
 * decoration layers (ticks, bands) window to the visible range.
 *
 * Scheduled cards render as bars on the canvas; non-terminal cards
 * with no due date land in an Unscheduled tray and can be dragged
 * onto the canvas to schedule. Bars drag whole (start + due) and
 * resize at either edge, snapped to days with a live date chip.
 */
import { computed, onUnmounted, ref, watch, watchEffect } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  addDays,
  addMonths,
  addWeeks,
  format,
  startOfMonth,
  startOfQuarter,
  startOfWeek,
} from 'date-fns'
import type { CardData } from '@nosdesk/core/sync/views/types'
import type { DependencyEdge } from '@nosdesk/core/services/dependenciesService'
import { TERMINAL_CATEGORIES, coarseStatusBucket } from '@nosdesk/core/types/workflow'
import { GANTT_ZOOMS, startOfDay, type GanttViewport } from '@/composables/useGanttViewport'
import EmptyState from '@/components/common/EmptyState.vue'
import Icon from '@/components/common/Icon.vue'
import HoverCard from '@/components/common/HoverCard.vue'
import UserAvatar from '@/components/UserAvatar.vue'
import { useHoverCard } from '@/composables/useHoverCard'
import GanttBarHoverCard from './GanttBarHoverCard.vue'
import TicketDragPreview from '@/components/common/TicketDragPreview.vue'
import { useDragDrop } from '@/sync/views/drag'
import { useBarDrag } from './useBarDrag'
import type { UseListGrouping } from '@/composables/useListGrouping'
import type { Density } from '@/composables/useTicketsDensity'
import {
  DENSITY_ROW_PX,
  GROUP_ROW_PX,
  BAR_INSET_Y,
  MIN_BAR_PX,
  LABEL_MIN_PX,
  AVATAR_MIN_PX,
} from './geometry'
import { buildRows, splitSchedule, type GanttRow, type ScheduledCard } from './rowModel'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

/** The slice of a cycle the board renders (bands + grouping labels).
 *  Structural, so both the REST DTO and the pool row satisfy it. */
export interface GanttCycle {
  id: number
  uuid: string
  name: string
  state: 'planned' | 'active' | 'completed'
  start_at?: string | null
  end_at?: string | null
}

const props = withDefaults(defineProps<{
  cards: readonly CardData[]
  edges?: readonly DependencyEdge[]
  /** Project cycles, rendered as shaded context bands behind the bars
   *  (only those with both a start and end date). */
  cycles?: readonly GanttCycle[]
  /** Time-scale viewport, owned by the route shell so the toolbar can
   *  live in the project tab bar. The renderer reads its refs for all
   *  geometry and reports content extent / visible count back to it. */
  viewport: GanttViewport
  /** Row grouping (cycle / state / assignee), owned by the route
   *  shell so the picker can ride the toolbar. Absent or set to
   *  'none' renders the flat list. */
  grouping?: UseListGrouping<ScheduledCard>
  /** Row density from the shared ListDensityToggle. */
  density?: Density
  onCardClick?: (cardId: number) => void
  /** Direct-manipulation write-back: bar body drag (moves both
   *  dates), either edge handle, or a tray drop (schedules a 1-day
   *  bar). Values are naive local-midnight datetimes; a missing key
   *  means "leave that date alone". Omitting the prop makes the
   *  board read-only. */
  onReschedule?: (
    cardId: number,
    patch: { start_date?: string; due_date?: string },
  ) => void
}>(), {
  edges: () => [],
  cycles: () => [],
  onCardClick: undefined,
  onReschedule: undefined,
})

// ===================== Time scale =====================
// The viewport (zoom + canvas + scroll position) is owned by the
// parent so the toolbar can sit in the project tab bar. Pull its refs
// out by name; they stay reactive because they're the same ref
// objects.
const vp = props.viewport
const { zoom, pxPerDay, canvasStart, canvasEnd, xOf, dateAt, totalWidth, scrollX, viewportWidth } = vp

/** Card-row height for the active density (group rows are fixed). */
const rowPx = computed(() => DENSITY_ROW_PX[props.density ?? 'comfortable'])

// ===================== Scheduled / unscheduled split =====================
// Row height + bar insets live in ./geometry; the schedule split and
// span resolution live in ./rowModel so the lane column and the
// timeline read one row source.

const schedule = computed(() => splitSchedule(props.cards))
const scheduled = computed<ScheduledCard[]>(() => schedule.value.scheduled)
const unscheduled = computed<CardData[]>(() => schedule.value.unscheduled)

// Grouped layout: the row model is the ONLY vertical geometry
// source; lane column and timeline iterate the same rows.
const bucketsRef = props.grouping ? props.grouping.buckets(scheduled) : null
const layout = computed(() =>
  buildRows(
    scheduled.value,
    bucketsRef?.value ?? [],
    (key) => props.grouping?.isCollapsed(key) ?? false,
    rowPx.value,
    GROUP_ROW_PX,
  ),
)
const ganttRows = computed<GanttRow[]>(() => layout.value.rows)

/** Group summary spans: min start to max end per group, kept when
 *  collapsed so the group retains its timeline scent. */
const groupSpans = computed(() =>
  ganttRows.value.flatMap((row) => {
    if (row.kind !== 'group' || !row.span) return []
    const left = xOf(row.span.start)
    return [
      {
        key: row.key,
        y: row.y,
        h: row.h,
        left,
        width: Math.max(MIN_BAR_PX, xOf(row.span.end) - left),
      },
    ]
  }),
)

const activeCycleKey = computed(() => {
  const active = props.cycles.find((c) => c.state === 'active')
  return active ? `cycle:${active.id}` : null
})

// Truly-empty project (no tickets at all) gets a centered empty state;
// distinct from "tickets exist but none land in the current window"
// (the pan/fit hint below the board).
const projectHasNoCards = computed(() => props.cards.length === 0)

// ===================== Viewport reporting =====================
// Feed the renderer's content extent and visible-bar count back to the
// shared viewport so the toolbar's Fit button and in-view label work
// without reaching into this component.
watchEffect(() => {
  const items = scheduled.value
  if (items.length === 0) {
    vp.setContentBounds(null)
    return
  }
  let min = items[0].start
  let max = items[0].end
  for (const it of items) {
    if (it.start.getTime() < min.getTime()) min = it.start
    if (it.end.getTime() > max.getTime()) max = it.end
  }
  vp.setContentBounds({ min, max })
})

// ===================== Axis: primary ticks + secondary band =====================

interface Tick {
  key: string
  x: number
  label: string
}
interface Band {
  key: string
  x: number
  width: number
  label: string
}

/** Decoration window: the visible date range plus a buffer each
 *  side, clamped to the canvas. Ticks / bands only materialise in
 *  here so a multi-year canvas at week zoom stays cheap; they're
 *  absolutely positioned, so windowing changes nothing visually. */
const decorationWindow = computed<{ start: Date; end: Date }>(() => {
  const bufferPx = Math.max(viewportWidth.value, 600)
  return {
    start: dateAt(Math.max(0, scrollX.value - bufferPx)),
    end: dateAt(Math.min(totalWidth.value, scrollX.value + viewportWidth.value + bufferPx)),
  }
})

/** Primary tick row: day (week zoom), week (month zoom, Monday
 *  aligned), or month (quarter zoom). */
const primaryTicks = computed<Tick[]>(() => {
  const out: Tick[] = []
  const start = decorationWindow.value.start
  const end = decorationWindow.value.end
  if (zoom.value === 'week') {
    let d = startOfDay(start)
    while (d.getTime() <= end.getTime()) {
      out.push({ key: d.toISOString(), x: xOf(d), label: format(d, 'd') })
      d = addDays(d, 1)
    }
  } else if (zoom.value === 'month') {
    let d = startOfWeek(start, { weekStartsOn: 1 })
    while (d.getTime() <= end.getTime()) {
      out.push({ key: d.toISOString(), x: xOf(d), label: format(d, 'd MMM') })
      d = addWeeks(d, 1)
    }
  } else {
    let d = startOfMonth(start)
    while (d.getTime() <= end.getTime()) {
      out.push({ key: d.toISOString(), x: xOf(d), label: format(d, 'MMM') })
      d = addMonths(d, 1)
    }
  }
  return out
})

/** Secondary band: month spans (week/month zoom) or quarter+year
 *  spans (quarter zoom). Width covers the band's days at pxPerDay. */
const secondaryBands = computed<Band[]>(() => {
  const out: Band[] = []
  const end = decorationWindow.value.end
  if (zoom.value === 'quarter') {
    let d = startOfQuarter(decorationWindow.value.start)
    while (d.getTime() <= end.getTime()) {
      const next = startOfQuarter(addMonths(d, 3))
      const x = xOf(d)
      const q = Math.floor(d.getMonth() / 3) + 1
      out.push({
        key: d.toISOString(),
        x,
        width: xOf(next) - x,
        label: `Q${q} ${d.getFullYear()}`,
      })
      d = next
    }
  } else {
    let d = startOfMonth(decorationWindow.value.start)
    while (d.getTime() <= end.getTime()) {
      const next = startOfMonth(addMonths(d, 1))
      const x = xOf(d)
      out.push({
        key: d.toISOString(),
        x,
        width: xOf(next) - x,
        label: format(d, 'MMM yyyy'),
      })
      d = next
    }
  }
  return out
})

/** Month zebra: alternating months get a faint wash across the axis
 *  and the body so the timeline has a readable monthly rhythm. */
interface ShadeBand {
  key: string
  x: number
  width: number
}
const zebraBands = computed<ShadeBand[]>(() => {
  const out: ShadeBand[] = []
  const end = decorationWindow.value.end
  let d = startOfMonth(decorationWindow.value.start)
  while (d.getTime() <= end.getTime()) {
    const next = startOfMonth(addMonths(d, 1))
    if (d.getMonth() % 2 === 1) {
      const x = xOf(d)
      out.push({ key: d.toISOString(), x, width: xOf(next) - x })
    }
    d = next
  }
  return out
})

/** Weekend ghosting at week / month zoom: a slightly denser wash
 *  over each Sat + Sun pair. Off at quarter zoom (too narrow). */
const weekendBands = computed<ShadeBand[]>(() => {
  if (zoom.value === 'quarter') return []
  const out: ShadeBand[] = []
  const end = decorationWindow.value.end
  let d = startOfWeek(decorationWindow.value.start, { weekStartsOn: 1 })
  while (d.getTime() <= end.getTime()) {
    const sat = addDays(d, 5)
    out.push({ key: sat.toISOString(), x: xOf(sat), width: pxPerDay.value * 2 })
    d = addWeeks(d, 1)
  }
  return out
})

const today = computed(() => startOfDay(new Date()))
const todayInRange = computed(
  () =>
    today.value.getTime() >= canvasStart.value.getTime() &&
    today.value.getTime() <= canvasEnd.value.getTime(),
)
const todayX = computed(() => xOf(today.value))

// ===================== Cycle bands =====================

interface CycleBand {
  key: string
  left: number
  width: number
  label: string
  state: GanttCycle['state']
}

/** Cycles with both dates become shaded bands spanning their range.
 *  end_at is inclusive, so the band runs through the end of that day. */
const cycleBands = computed<CycleBand[]>(() => {
  const max = totalWidth.value
  const out: CycleBand[] = []
  for (const c of props.cycles) {
    if (!c.start_at || !c.end_at) continue
    const rawLeft = xOf(startOfDay(new Date(c.start_at)))
    const rawRight = xOf(addDays(startOfDay(new Date(c.end_at)), 1))
    if (rawRight <= 0 || rawLeft >= max) continue
    const left = Math.max(0, rawLeft)
    out.push({
      key: c.uuid,
      left,
      width: Math.max(1, Math.min(max, rawRight) - left),
      label: c.name,
      state: c.state,
    })
  }
  return out
})

/** Strip-label tint by cycle state: active stands out, planned is
 *  neutral, completed is muted. */
function cycleStripClass(state: GanttCycle['state']): string {
  if (state === 'active') return 'bg-accent/15 text-accent border-accent/40'
  if (state === 'planned') return 'bg-surface-hover text-secondary border-subtle'
  return 'bg-surface-alt text-tertiary border-subtle'
}

/** Body shading: a faint wash so the band reads behind the bars
 *  without competing with them. */
function cycleBodyClass(state: GanttCycle['state']): string {
  return state === 'active' ? 'bg-accent/5' : 'bg-surface-hover/30'
}

// ===================== Bars =====================

interface BarRow {
  card: CardData
  /** Row top (px), from the shared row model. */
  y: number
  left: number
  width: number
  terminal: boolean
  /** Non-terminal and past its due date: an at-risk (overdue) bar. */
  atRisk: boolean
  /** True right edge, for arrow anchoring. */
  rightX: number
  /** Effective span (with any live drag preview applied). */
  start: Date
  end: Date
}

// The one scroll container (both axes) and the sticky lane column.
// The viewport owns their scroll/size wiring.
const scrollerEl = ref<HTMLElement | null>(null)
const laneColEl = ref<HTMLElement | null>(null)
// The timeline body grid cell: the geometry origin for the resize
// drag (its rect's left edge is canvas x = 0 in client coords).
const bodyEl = ref<HTMLElement | null>(null)

watch(
  [scrollerEl, laneColEl],
  ([scroller, lane]) => vp.attachScroller(scroller, lane),
  { immediate: true },
)
onUnmounted(() => vp.attachScroller(null))

/** Naive local-midnight datetime (no tz suffix). Dates round-trip
 *  through the backend's NaiveDateTime model, whose deserialiser
 *  rejects a trailing `Z`; sending the local day also keeps the bar
 *  anchored to the day the user dropped it on. */
function naiveDay(d: Date): string {
  return `${format(d, 'yyyy-MM-dd')}T00:00:00`
}

// Declared before `bars` because the visibleCount watchEffect below
// evaluates that computed synchronously at setup.
const barDrag = useBarDrag({
  pxPerDay,
  canvasStart,
  bodyEl,
  scroller: scrollerEl,
  onDragStart: () => hover.dismiss(),
  onCommit: ({ cardId, mode, start, end }) => {
    if (mode === 'due') props.onReschedule?.(cardId, { due_date: naiveDay(end) })
    else if (mode === 'start') props.onReschedule?.(cardId, { start_date: naiveDay(start) })
    else
      props.onReschedule?.(cardId, {
        start_date: naiveDay(start),
        due_date: naiveDay(end),
      })
  },
})

// Bars render at their true canvas coordinates, never dropped or
// clamped: rows stay stable while scrolling, and arrows to off-screen
// bars keep both endpoints. Vertical geometry (y) comes from the
// shared row model, which the lane column iterates too, so lane and
// timeline align by construction.
const bars = computed<BarRow[]>(() => {
  const out: BarRow[] = []
  const p = barDrag.preview.value
  for (const row of ganttRows.value) {
    if (row.kind !== 'card') continue
    const it = row.sched
    const dragged = p && p.cardId === it.card.id
    const start = dragged ? p.start : it.start
    const end = dragged ? p.end : it.end
    const left = xOf(start)
    const right = xOf(end)
    const width = Math.max(pxPerDay.value, right - left)
    const terminal = TERMINAL_CATEGORIES.has(it.card.workflow_state.category)
    out.push({
      card: it.card,
      y: row.y,
      left,
      width,
      terminal,
      atRisk: !terminal && today.value.getTime() > end.getTime(),
      rightX: right,
      start,
      end,
    })
  }
  return out
})

// Report the count of bars intersecting the visible window for the
// toolbar's in-view label.
watchEffect(() => {
  const startX = scrollX.value
  const endX = startX + viewportWidth.value
  vp.visibleCount.value = bars.value.filter((b) => b.rightX >= startX && b.left <= endX).length
})

function zoomStep(dir: 1 | -1): GanttZoomStep {
  const idx = GANTT_ZOOMS.indexOf(zoom.value)
  return GANTT_ZOOMS[Math.min(GANTT_ZOOMS.length - 1, Math.max(0, idx + dir))]
}
type GanttZoomStep = (typeof GANTT_ZOOMS)[number]

// Ctrl/cmd + wheel: zoom anchored at the cursor (trackpad pinch
// arrives as a ctrlKey wheel event). Plain wheel scrolls natively.
function onWheel(event: WheelEvent): void {
  if (!event.ctrlKey && !event.metaKey) return
  event.preventDefault()
  const next = zoomStep(event.deltaY > 0 ? 1 : -1)
  if (next === zoom.value) return
  const scroller = scrollerEl.value
  if (!scroller) return
  const laneW = laneColEl.value?.offsetWidth ?? 0
  const anchorPx = event.clientX - scroller.getBoundingClientRect().left - laneW
  vp.setZoom(next, Math.max(0, anchorPx))
}

// ===================== Keyboard =====================
// Board shortcuts: t today, f fit, [ ] pan, - = zoom. Bars are real
// buttons (tab order); Arrow Up/Down walks rows, Shift+Arrow nudges
// the focused bar's due date a day with a polite announcement.
const announcement = ref('')

function onBoardKeydown(event: KeyboardEvent): void {
  const target = event.target as HTMLElement | null
  if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)) return
  if (event.metaKey || event.ctrlKey || event.altKey) return
  switch (event.key) {
    case 't':
      vp.centerOnToday()
      break
    case 'f':
      vp.fitToProject()
      break
    case '[':
      vp.pan(-1)
      break
    case ']':
      vp.pan(1)
      break
    case '=':
    case '+':
      vp.setZoom(zoomStep(-1))
      break
    case '-':
      vp.setZoom(zoomStep(1))
      break
    default:
      return
  }
  event.preventDefault()
}

function focusBarByOffset(currentId: number, offset: number): void {
  const list = bars.value
  const idx = list.findIndex((b) => b.card.id === currentId)
  const next = list[idx + offset]
  if (idx === -1 || !next) return
  scrollerEl.value
    ?.querySelector<HTMLElement>(`[data-bar-id="${next.card.id}"]`)
    ?.focus()
}

function onBarKeydown(row: BarRow, event: KeyboardEvent): void {
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault()
    focusBarByOffset(row.card.id, event.key === 'ArrowDown' ? 1 : -1)
    return
  }
  if (
    event.shiftKey &&
    (event.key === 'ArrowLeft' || event.key === 'ArrowRight') &&
    props.onReschedule &&
    !row.terminal
  ) {
    event.preventDefault()
    const newEnd = addDays(row.end, event.key === 'ArrowRight' ? 1 : -1)
    if (newEnd.getTime() <= row.start.getTime()) return
    props.onReschedule(row.card.id, { due_date: naiveDay(newEnd) })
    announcement.value = t('gantt-nudge-announce', {
      title: row.card.title,
      date: format(newEnd, 'MMM d'),
    })
  }
}

// ===================== Direct manipulation =====================

function beginBarDrag(mode: 'move' | 'start' | 'due', bar: BarRow, event: PointerEvent): void {
  if (!props.onReschedule || bar.terminal) return
  if (mode !== 'move') event.stopPropagation()
  barDrag.begin(mode, { cardId: bar.card.id, start: bar.start, end: bar.end }, event)
}

/** Ghost + date chip geometry for the active bar drag. */
const dragChrome = computed(() => {
  const p = barDrag.preview.value
  if (!p) return null
  const row = bars.value.find((b) => b.card.id === p.cardId)
  if (!row) return null
  const activeDate = p.mode === 'start' ? p.start : p.end
  return {
    y: row.y,
    ghostLeft: xOf(p.origStart),
    ghostWidth: Math.max(MIN_BAR_PX, xOf(p.origEnd) - xOf(p.origStart) - 4),
    chipX: xOf(activeDate),
    chipLabel: format(activeDate, 'MMM d'),
  }
})

// Tray -> canvas scheduling: the shared lane-drop machinery with
// "lane" = the ISO day under the pointer. A drop schedules the
// ticket as a 1-day bar (start = due = day) the user can stretch.
const trayDrag = useDragDrop({
  resolveLaneAt: (x, y) => {
    const body = bodyEl.value
    if (!body || !props.onReschedule) return null
    const rect = body.getBoundingClientRect()
    if (x < rect.left || x > rect.right || y < rect.top || y > rect.bottom) return null
    const day = Math.round((x - rect.left) / pxPerDay.value)
    return format(addDays(canvasStart.value, day), 'yyyy-MM-dd')
  },
  onDrop: ({ cardIds, targetLane }) => {
    for (const id of cardIds) {
      props.onReschedule?.(id, {
        start_date: `${targetLane}T00:00:00`,
        due_date: `${targetLane}T00:00:00`,
      })
    }
  },
  onClick: (cardId) => props.onCardClick?.(cardId),
  getEdgeScrollTargets: () =>
    scrollerEl.value ? [{ el: scrollerEl.value, axes: 'both' as const }] : [],
})

const trayDraggedCard = computed<CardData | null>(() => {
  const id = trayDrag.state.draggedCardIds[0]
  if (id == null) return null
  return unscheduled.value.find((c) => c.id === id) ?? null
})

/** Drop-day highlight while dragging a tray ticket over the canvas. */
const trayDropX = computed<number | null>(() => {
  const lane = trayDrag.state.hoverLane
  if (!lane || !trayDrag.state.isDragging) return null
  return xOf(startOfDay(new Date(`${lane}T00:00:00`)))
})

// Height of the rendered rows. Driven by the in-window bars (which the
// lane column also iterates), so lane and timeline stay the same height
// in the frozen-pane grid.
const totalHeight = computed(() => layout.value.totalHeight)

/** Soft status fill: the coarse open / in-progress / done bucket as a
 *  muted tint + matching subtle border. Status drives the fill (not
 *  priority) so a bar reads its state at a glance. */
function barStatusClass(card: CardData): string {
  switch (coarseStatusBucket(card.workflow_state.category)) {
    case 'open':
      return 'bg-status-open-muted border-status-open/40'
    case 'in-progress':
      return 'bg-status-in-progress-muted border-status-in-progress/40'
    default:
      return 'bg-status-closed-muted border-status-closed/40'
  }
}

/** Priority as a thin left accent edge, not the whole fill, so priority
 *  and status stay separable signals. 'none' gets no edge. */
function priorityEdgeClass(p: CardData['priority']): string {
  if (p === 'urgent' || p === 'high') return 'border-l-[3px] border-l-priority-high'
  if (p === 'medium') return 'border-l-[3px] border-l-priority-medium'
  if (p === 'low') return 'border-l-[3px] border-l-priority-low'
  return ''
}

/** Elapsed-time ("consumed") width inside a bar: from its left edge to
 *  today, clamped to the bar. A desaturated shade on non-terminal bars,
 *  so it reads as schedule pressure, not work-done. */
function scheduleFillWidth(row: BarRow): number {
  if (row.terminal) return 0
  return Math.max(0, Math.min(row.width, todayX.value - row.left))
}

/** Accessible name for a bar: title + span. The rich detail lives
 *  in the hover card, which is supplementary. */
function barAriaLabel(b: BarRow): string {
  return `${b.card.title}, ${format(b.start, 'MMM d')} - ${format(b.end, 'MMM d')}`
}

// ===================== Hover card =====================
// One shared instance for every bar: enter/leave (and focus) on a
// bar retargets the same card, so 200 bars cost two extra DOM nodes,
// not 200 popovers.
const hover = useHoverCard<BarRow>()

const cycleNameById = computed(() => {
  const map = new Map<number, string>()
  for (const c of props.cycles) map.set(c.id, c.name)
  return map
})

function onBarEnter(row: BarRow, event: Event): void {
  hover.onTargetEnter(event.currentTarget as HTMLElement, row)
}

// ===================== Arrows =====================

interface Arrow {
  key: string
  fromX: number
  fromY: number
  toX: number
  toY: number
}

const arrows = computed<Arrow[]>(() => {
  const byId = new Map<number, BarRow>()
  for (const b of bars.value) byId.set(b.card.id, b)
  const out: Arrow[] = []
  for (const e of props.edges) {
    if (e.relation_type !== 'blocks') continue
    const src = byId.get(e.from)
    const dst = byId.get(e.to)
    if (!src || !dst) continue
    out.push({
      key: `${e.from}->${e.to}`,
      fromX: src.rightX,
      fromY: src.y + rowPx.value / 2,
      toX: dst.left,
      toY: dst.y + rowPx.value / 2,
    })
  }
  return out
})

function arrowPath(a: Arrow): string {
  const dx = Math.max(20, Math.abs(a.toX - a.fromX) * 0.4)
  return `M${a.fromX},${a.fromY} C${a.fromX + dx},${a.fromY} ${a.toX - dx},${a.toY} ${a.toX},${a.toY}`
}

// ===================== Tray =====================

const trayOpen = ref(true)

function open(card: CardData): void {
  // A completed bar drag releases on the bar and would otherwise
  // read as a click; swallow it.
  if (barDrag.shouldSuppressClick()) return
  props.onCardClick?.(card.id)
}
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Truly-empty project: one centered state, no grid scaffold to
         leak through. (Distinct from "tickets exist but none in this
         window", which is the in-body hint.) -->
    <EmptyState
      v-if="projectHasNoCards"
      class="flex-1"
      icon="calendar"
      :title="t('gantt-empty-title')"
      :description="t('gantt-empty-description')"
    />

    <!-- Frozen-pane grid on one two-axis scroll container: lane column
         sticky left, axis header sticky top, corner sticky on both.
         No absolute overlay panel, no LEFT_PX margin offset; lane rows
         and timeline bars are the SAME grid rows, so they align by
         construction. The timeline column is the full canvas width
         (the viewport derives the range from content bounds), so
         horizontal navigation is native scrolling.

         Lane width is the --lane-w CSS var, scaled by container width.
         A flat 200px ate more than half a 390px phone and left ~190px of
         actual timeline, which is not enough to read a schedule against;
         140px keeps titles legible (they truncate, with a `title`) while
         roughly doubling the timeline. `useGanttViewport` measures the
         lane element rather than assuming a constant, so the viewport
         maths follows this automatically. -->
    <div
      v-else
      ref="scrollerEl"
      class="grid flex-1 min-h-0 overflow-auto outline-none [--lane-w:140px] @lg:[--lane-w:200px] @3xl:[--lane-w:240px]"
      :style="{
        gridTemplateColumns: `var(--lane-w) ${totalWidth}px`,
        gridTemplateRows: 'auto 1fr',
      }"
      tabindex="0"
      role="application"
      :aria-label="t('gantt-board-label')"
      @wheel="onWheel"
      @keydown="onBoardKeydown"
    >
      <!-- Corner: the lane's header cell, pinned on both axes. -->
      <div class="sticky top-0 left-0 z-30 bg-surface border-b border-r border-subtle"></div>

      <!-- Axis header (months / days / cycles), pinned on vertical scroll. -->
      <div class="sticky top-0 z-20 relative bg-surface border-b border-subtle">
        <!-- Month zebra behind the header rows, matching the body's
             rhythm so header and canvas read as one system. -->
        <div class="absolute inset-0 overflow-hidden pointer-events-none">
          <div
            v-for="band in zebraBands"
            :key="`hz-${band.key}`"
            class="absolute top-0 bottom-0 bg-surface-alt/40"
            :style="{ left: `${band.x}px`, width: `${band.width}px` }"
          ></div>
        </div>
        <!-- Secondary band (month / quarter spans). -->
        <div
          class="relative h-6 text-[11px] uppercase tracking-wide font-semibold text-tertiary border-b border-subtle/60"
        >
          <div
            v-for="b in secondaryBands"
            :key="b.key"
            class="absolute top-0 bottom-0 flex items-center px-2 border-r border-subtle/50 overflow-hidden whitespace-nowrap"
            :style="{ left: `${Math.max(0, b.x)}px`, width: `${b.width + Math.min(0, b.x)}px` }"
          >{{ b.label }}</div>
        </div>
        <!-- Primary tick row (day / week), with a labelled Today pill
             anchored on the today line. -->
        <div
          class="relative h-6 text-[11px] tabular-nums text-tertiary"
          :class="cycleBands.length > 0 ? 'border-b border-subtle/60' : ''"
        >
          <div
            v-for="tick in primaryTicks"
            :key="tick.key"
            class="absolute top-0 bottom-0 flex items-center pl-1 border-l border-subtle/30 whitespace-nowrap"
            :style="{ left: `${tick.x}px` }"
          >{{ tick.label }}</div>
          <span
            v-if="todayInRange"
            class="absolute top-1/2 -translate-y-1/2 -translate-x-1/2 z-10 rounded-full bg-accent px-1.5 py-px text-[11px] leading-4 font-semibold text-on-accent whitespace-nowrap pointer-events-none"
            :style="{ left: `${todayX}px` }"
          >{{ t('gantt-today') }}</span>
        </div>
        <!-- Cycle strip: one labelled segment per dated cycle. -->
        <div v-if="cycleBands.length > 0" class="relative h-6">
          <div
            v-for="band in cycleBands"
            :key="band.key"
            class="absolute top-0 bottom-0 flex items-center px-2 border-l border-r text-[11px] font-medium truncate"
            :class="cycleStripClass(band.state)"
            :style="{ left: `${band.left}px`, width: `${band.width}px` }"
            :title="band.label"
          >{{ band.label }}</div>
        </div>
      </div>

      <!-- Lane column: group headers + ticket titles + the unscheduled
           tray, iterating the SAME row model as the timeline body.
           Sticky left with an opaque background so bars pass beneath. -->
      <div ref="laneColEl" class="sticky left-0 z-10 border-r border-subtle bg-app">
        <template v-for="row in ganttRows" :key="row.kind === 'group' ? `g-${row.key}` : row.sched.card.id">
          <button
            v-if="row.kind === 'group'"
            type="button"
            class="w-full flex items-center gap-1.5 px-2 text-xs font-medium text-secondary bg-surface-alt border-b border-subtle/60 hover:bg-surface-hover focus:outline-none focus-visible:bg-surface-hover"
            :style="{ height: `${row.h}px` }"
            :aria-expanded="!row.collapsed"
            @click="grouping?.toggleCollapsed(row.key)"
          >
            <Icon
              name="chevronRight"
              size="xs"
              class="text-tertiary transition-transform shrink-0"
              :class="row.collapsed ? '' : 'rotate-90'"
            />
            <span class="truncate">{{ row.label }}</span>
            <span class="text-tertiary tabular-nums">{{ row.count }}</span>
          </button>
          <button
            v-else
            type="button"
            class="w-full flex items-center gap-2 px-3 text-xs text-left text-primary border-b border-subtle/50 hover:bg-surface-hover focus:outline-none focus-visible:bg-surface-hover"
            :style="{ height: `${row.h}px` }"
            :title="row.sched.card.title"
            @click="open(row.sched.card)"
          >
            <span class="font-mono text-tertiary shrink-0">#{{ row.sched.card.id }}</span>
            <span class="truncate">{{ row.sched.card.title }}</span>
          </button>
        </template>

        <!-- Unscheduled tray -->
        <div v-if="unscheduled.length > 0" class="border-t border-subtle">
          <button
            type="button"
            class="w-full flex items-center gap-2 px-3 py-2 text-xs font-medium text-secondary hover:bg-surface-hover"
            @click="trayOpen = !trayOpen"
          >
            <Icon
              name="chevronRight"
              size="xs"
              class="text-tertiary transition-transform"
              :class="trayOpen ? 'rotate-90' : ''"
            />
            {{ t('gantt-unscheduled', { count: unscheduled.length }) }}
          </button>
          <div v-if="trayOpen" class="max-h-48 overflow-auto">
            <!-- Tray rows drag onto the canvas to schedule (drop day
                 becomes start + due); a plain click still opens. -->
            <button
              v-for="card in unscheduled"
              :key="card.id"
              type="button"
              class="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-left text-primary border-t border-subtle/40 hover:bg-surface-hover focus:outline-none focus-visible:bg-surface-hover"
              :class="[
                onReschedule ? 'cursor-grab' : '',
                trayDrag.isDraggedCard(card.id) ? 'opacity-50' : '',
              ]"
              :title="card.title"
              @pointerdown="trayDrag.onPointerDown(card.id, $event)"
            >
              <span class="font-mono text-tertiary shrink-0">#{{ card.id }}</span>
              <span class="truncate">{{ card.title }}</span>
            </button>
          </div>
        </div>
      </div>

      <!-- Timeline body: the only absolutely-positioned surface, and only
           within its own grid cell (no global offset). Its rect's left
           edge is canvas x = 0 for the resize drag. -->
      <div
        ref="bodyEl"
        class="self-start relative overflow-hidden"
        :style="{ height: `${Math.max(totalHeight, 100)}px`, width: `${totalWidth}px` }"
      >
        <!-- Month zebra (canvas rhythm, matches the header's). -->
        <div
          v-for="band in zebraBands"
          :key="`bz-${band.key}`"
          class="absolute top-0 bottom-0 bg-surface-alt/40 pointer-events-none"
          :style="{ left: `${band.x}px`, width: `${band.width}px` }"
        ></div>

        <!-- Weekend ghosting (week / month zoom). -->
        <div
          v-for="band in weekendBands"
          :key="`wk-${band.key}`"
          class="absolute top-0 bottom-0 bg-surface-alt/70 pointer-events-none"
          :style="{ left: `${band.x}px`, width: `${band.width}px` }"
        ></div>

        <!-- Gridlines derived from the primary ticks: one source of
             truth with the axis labels, so the lines always align
             (the old repeating-gradient drifted off the calendar). -->
        <div
          v-for="tick in primaryTicks"
          :key="`grid-${tick.key}`"
          class="absolute top-0 bottom-0 w-px bg-(--color-subtle)/60 pointer-events-none"
          :style="{ left: `${tick.x}px` }"
        ></div>

        <!-- Cycle band shading (behind bars) -->
        <div
          v-for="band in cycleBands"
          :key="`shade-${band.key}`"
          class="absolute top-0 bottom-0 border-l border-r border-subtle/60"
          :class="cycleBodyClass(band.state)"
          :style="{ left: `${band.left}px`, width: `${band.width}px` }"
        ></div>

        <!-- Group summary spans: a quiet band from the group's first
             start to its last end. Clicking toggles the fold, same as
             the lane header. -->
        <button
          v-for="span in groupSpans"
          :key="`span-${span.key}`"
          type="button"
          class="absolute rounded-md border border-subtle cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          :class="span.key === activeCycleKey ? 'bg-accent/10' : 'bg-surface-hover/60'"
          :style="{
            left: `${span.left}px`,
            width: `${span.width}px`,
            top: `${span.y + 7}px`,
            height: `${span.h - 14}px`,
          }"
          :aria-label="t('gantt-group-span-label')"
          @click="grouping?.toggleCollapsed(span.key)"
        ></button>

        <!-- Today marker: an accent line; the labelled pill lives in
             the axis header above. -->
        <div
          v-if="todayInRange"
          class="absolute top-0 bottom-0 w-px bg-accent z-[5] pointer-events-none"
          :style="{ left: `${todayX}px` }"
        ></div>

        <!-- Drag chrome: ghost outline at the original span + a
             snapped date chip riding the active edge. -->
        <template v-if="dragChrome">
          <div
            class="absolute rounded-md border border-dashed border-strong/60 pointer-events-none z-[3]"
            :style="{
              left: `${dragChrome.ghostLeft}px`,
              width: `${dragChrome.ghostWidth}px`,
              top: `${dragChrome.y + BAR_INSET_Y}px`,
              height: `${rowPx - BAR_INSET_Y * 2}px`,
            }"
          ></div>
          <span
            class="absolute -translate-x-1/2 rounded-md bg-surface border border-default shadow-sm px-1.5 py-0.5 text-[11px] tabular-nums text-primary whitespace-nowrap pointer-events-none z-20"
            :style="{
              left: `${dragChrome.chipX}px`,
              top: `${Math.max(0, dragChrome.y - 22)}px`,
            }"
          >{{ dragChrome.chipLabel }}</span>
        </template>

        <!-- Drop-day highlight while dragging a tray ticket in. -->
        <div
          v-if="trayDropX != null"
          class="absolute top-0 bottom-0 bg-accent/10 border-x border-accent/40 pointer-events-none z-[4]"
          :style="{ left: `${trayDropX}px`, width: `${Math.max(pxPerDay, 8)}px` }"
        ></div>

        <!-- Dependency arrows -->
        <svg
          :width="totalWidth"
          :height="Math.max(totalHeight, 100)"
          class="absolute inset-0 pointer-events-none"
        >
          <defs>
            <marker
              id="gantt-arrowhead"
              viewBox="0 0 8 8"
              refX="7"
              refY="4"
              markerWidth="8"
              markerHeight="8"
              orient="auto"
            >
              <path d="M0,0 L8,4 L0,8 z" fill="currentColor" />
            </marker>
          </defs>
          <g class="text-accent">
            <path
              v-for="a in arrows"
              :key="a.key"
              :d="arrowPath(a)"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              marker-end="url(#gantt-arrowhead)"
            />
          </g>
        </svg>

        <!-- Bars -->
        <template v-for="row in bars" :key="row.card.id">
          <button
            type="button"
            class="group/bar motion-safe:transition-[transform,box-shadow,filter] motion-safe:duration-150 absolute flex items-center rounded-md border overflow-hidden text-left cursor-pointer hover:-translate-y-px hover:shadow-sm hover:brightness-[1.03] focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:z-10"
            :class="[
              barStatusClass(row.card),
              priorityEdgeClass(row.card.priority),
              row.atRisk ? 'ring-1 ring-status-error/60' : '',
              row.terminal ? 'gantt-bar-terminal' : '',
              onReschedule && !row.terminal ? 'cursor-grab active:cursor-grabbing' : '',
            ]"
            :style="{
              left: `${row.left}px`,
              width: `${Math.max(MIN_BAR_PX, row.width - 4)}px`,
              top: `${row.y + BAR_INSET_Y}px`,
              height: `${rowPx - BAR_INSET_Y * 2}px`,
            }"
            :aria-label="barAriaLabel(row)"
            :data-bar-id="row.card.id"
            @click="open(row.card)"
            @keydown="onBarKeydown(row, $event)"
            @pointerdown="beginBarDrag('move', row, $event)"
            @pointerenter="onBarEnter(row, $event)"
            @pointerleave="hover.onTargetLeave()"
            @focusin="onBarEnter(row, $event)"
            @focusout="hover.onTargetLeave()"
          >
            <!-- Schedule (elapsed-time) fill: a desaturated 'consumed'
                 shade from the bar's start to today. Reads as schedule
                 pressure, not work done; error-tinted when overdue. -->
            <span
              v-if="scheduleFillWidth(row) > 0"
              class="absolute inset-y-0 left-0 pointer-events-none"
              :class="row.atRisk ? 'bg-status-error/10' : 'bg-black/[0.06] dark:bg-white/[0.07]'"
              :style="{ width: `${scheduleFillWidth(row)}px` }"
            ></span>
            <!-- Done check on finished bars (width permitting). -->
            <Icon
              v-if="row.terminal && row.width >= 32"
              name="check"
              size="xs"
              class="relative z-[1] ml-1.5 shrink-0 text-tertiary"
            />
            <!-- Assignee avatar inside the bar when it fits. -->
            <UserAvatar
              v-if="row.card.assignee_uuid && row.width >= AVATAR_MIN_PX"
              :uuid="row.card.assignee_uuid"
              size="xxs"
              :show-name="false"
              :clickable="false"
              class="relative z-[1] ml-1.5 shrink-0"
            />
            <!-- At-risk dot keeps the overdue signal visible when the
                 bar is too narrow for the tinted fill to read. -->
            <span
              v-if="row.atRisk && row.width < LABEL_MIN_PX"
              class="relative z-[1] ml-1 h-1.5 w-1.5 shrink-0 rounded-full bg-status-error"
            ></span>
            <span
              v-if="row.width >= LABEL_MIN_PX"
              class="relative z-[1] px-1.5 text-[11px] text-primary truncate"
            >
              {{ row.card.title }}
            </span>
            <!-- Edge handles (open bars only). Left edits start_date
                 (grabbing it on a created_at-fallback bar promotes the
                 ticket to a planned start); right edits due_date.
                 Grips become visible on bar hover. -->
            <span
              v-if="onReschedule && !row.terminal"
              class="absolute top-0 bottom-0 left-0 w-2.5 cursor-ew-resize hover:bg-accent/40 z-[2] flex items-center justify-center touch-none"
              @pointerdown="beginBarDrag('start', row, $event)"
              @click.stop
            >
              <span
                class="h-3 w-px bg-strong opacity-0 group-hover/bar:opacity-60 transition-opacity"
              ></span>
            </span>
            <span
              v-if="onReschedule && !row.terminal"
              class="absolute top-0 bottom-0 right-0 w-2.5 cursor-ew-resize hover:bg-accent/40 z-[2] flex items-center justify-center touch-none"
              @pointerdown="beginBarDrag('due', row, $event)"
              @click.stop
            >
              <span
                class="h-3 w-px bg-strong opacity-0 group-hover/bar:opacity-60 transition-opacity"
              ></span>
            </span>
          </button>
          <!-- Narrow bars float their title outside, to the right,
               instead of truncating into nothing (industry idiom). -->
          <span
            v-if="row.width < LABEL_MIN_PX"
            class="absolute text-[11px] text-secondary whitespace-nowrap pointer-events-none"
            :style="{
              left: `${row.left + Math.max(MIN_BAR_PX, row.width - 4) + 6}px`,
              top: `${row.y + BAR_INSET_Y}px`,
              lineHeight: `${rowPx - BAR_INSET_Y * 2}px`,
            }"
          >{{ row.card.title }}</span>
        </template>

        <!-- Tickets exist, but nothing is scheduled (all in the tray).
             Anchored to the visible window, not the canvas, so the
             hint stays centered while scrolling.

             The width is the MEASURED visible timeline (`viewportWidth`
             already nets off the lane column). It used to carry a 240px
             floor, which on a phone exceeded the ~190px actually on screen:
             the box overflowed to the right and clipped its own centred
             text mid-sentence. The floor now only covers the first paint,
             before the scroller has been measured. -->
        <div
          v-if="bars.length === 0"
          class="absolute inset-y-0 flex flex-col items-center justify-center gap-3 px-4 text-center"
          :style="{ left: `${scrollX}px`, width: `${viewportWidth > 0 ? viewportWidth : 240}px` }"
        >
          <p class="text-sm text-tertiary max-w-sm">{{ t('gantt-nothing-scheduled') }}</p>
        </div>
      </div>
    </div>

    <!-- Polite announcements for keyboard nudges. -->
    <span class="sr-only" aria-live="polite">{{ announcement }}</span>

    <!-- Floating preview while dragging a tray ticket. -->
    <TicketDragPreview
      v-if="trayDrag.state.isDragging && trayDraggedCard && trayDrag.state.dragPosition"
      :ticket="{
        id: trayDraggedCard.id,
        title: trayDraggedCard.title,
        category: trayDraggedCard.workflow_state.category,
        assigneeUuid: trayDraggedCard.assignee_uuid,
        priority: trayDraggedCard.priority === 'urgent' ? 'high' : trayDraggedCard.priority,
      }"
      :position="trayDrag.state.dragPosition"
      :extra-count="trayDrag.state.draggedCardIds.length - 1"
    />

    <!-- One shared hover card for every bar (teleported to body). -->
    <HoverCard
      :open="hover.open.value"
      :anchor="hover.anchorEl.value"
      placement="top-start"
      @close="hover.dismiss()"
      @card-enter="hover.onCardEnter()"
      @card-leave="hover.onCardLeave()"
    >
      <GanttBarHoverCard
        v-if="hover.payload.value"
        :card="hover.payload.value.card"
        :start="hover.payload.value.start"
        :end="hover.payload.value.end"
        :cycle-name="hover.payload.value.card.cycle_id != null
          ? (cycleNameById.get(hover.payload.value.card.cycle_id) ?? null)
          : null"
        :resizable="!!onReschedule && !hover.payload.value.terminal"
      />
    </HoverCard>
  </div>
</template>

<style scoped>
/* Terminal-category bars read as finished: muted + slightly
   desaturated, dropping the busy diagonal stripe for a cleaner look. */
.gantt-bar-terminal {
  opacity: 0.6;
  filter: saturate(0.7);
}
</style>
