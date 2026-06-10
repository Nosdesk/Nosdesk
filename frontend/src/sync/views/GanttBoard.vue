<script setup lang="ts">
/**
 * Zoom-aware Gantt renderer.
 *
 * A horizontal time scale (Week / Month / Quarter) drives every
 * geometry. `xOf(date)` is the single projection from a date to a
 * pixel offset; bars, axis ticks, the today line, and dependency
 * arrows all read through it, so the whole board is a pure function
 * of (cards, edges, [rangeStart, rangeEnd], pxPerDay).
 *
 * Scheduled cards render as bars on the canvas; non-terminal cards
 * with no due date land in an Unscheduled tray instead. Dragging
 * bars / tray items to reschedule is a follow-up.
 */
import { computed, onMounted, onUnmounted, ref, watchEffect } from 'vue'
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
import type { CardData } from './types'
import type { DependencyEdge } from '@/services/dependenciesService'
import type { Cycle } from '@/services/cyclesService'
import { TERMINAL_CATEGORIES, coarseStatusBucket } from '@/types/workflow'
import { startOfDay, type GanttViewport } from '@/composables/useGanttViewport'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const props = withDefaults(defineProps<{
  cards: readonly CardData[]
  edges?: readonly DependencyEdge[]
  /** Project cycles, rendered as shaded context bands behind the bars
   *  (only those with both a start and end date). */
  cycles?: readonly Cycle[]
  /** Time-scale viewport, owned by the route shell so the toolbar can
   *  live in the project tab bar. The renderer reads its refs for all
   *  geometry and reports content extent / visible count back to it. */
  viewport: GanttViewport
  onCardClick?: (cardId: number) => void
  /** Drag-the-due-handle write-back. Called with the new due date
   *  (ISO) when the user releases a bar's right handle. Only the due
   *  (right) edge moves; created_at is immutable history. Omitting
   *  this hides the resize handle. */
  onReschedule?: (cardId: number, dueDate: string) => void
}>(), {
  edges: () => [],
  cycles: () => [],
  onCardClick: undefined,
  onReschedule: undefined,
})

// ===================== Time scale =====================
// The viewport (zoom + visible window) is owned by the parent so the
// toolbar can sit in the project tab bar. Pull its refs out by name so
// the geometry below reads exactly as before; they stay reactive
// because they're the same ref objects.
const vp = props.viewport
const { zoom, pxPerDay, rangeStart, rangeEnd, xOf, totalWidth } = vp

// Comfortable density default (Phase 1 of the gantt/cycle design
// overhaul). A density toggle to a compact ~28px row is a follow-up.
const ROW_PX = 40
const LEFT_PX = 240

// ===================== Scheduled / unscheduled split =====================

interface ScheduledCard {
  card: CardData
  start: Date
  end: Date
}

function isUnscheduled(card: CardData): boolean {
  const terminal = TERMINAL_CATEGORIES.has(card.workflow_state.category)
  return !terminal && !card.due_date
}

/** Resolve a scheduled card's [start, end], clamping end to at
 *  least start + 1 day so a same-day bar still has width. */
function spanOf(card: CardData): { start: Date; end: Date } {
  const start = startOfDay(new Date(card.created_at))
  const terminal = TERMINAL_CATEGORIES.has(card.workflow_state.category)
  let end: Date
  if (card.due_date) {
    end = startOfDay(new Date(card.due_date))
  } else if (terminal) {
    // No due date on a finished card: use its lifespan as a best
    // effort (created to last update).
    end = startOfDay(new Date(card.updated_at))
  } else {
    end = startOfDay(new Date())
  }
  if (end.getTime() <= start.getTime()) end = addDays(start, 1)
  return { start, end }
}

const scheduled = computed<ScheduledCard[]>(() => {
  const out: ScheduledCard[] = []
  for (const card of props.cards) {
    if (isUnscheduled(card)) continue
    if (!card.created_at) continue
    const { start, end } = spanOf(card)
    out.push({ card, start, end })
  }
  return out
})

const unscheduled = computed<CardData[]>(() =>
  props.cards.filter(isUnscheduled),
)

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

// Scroll container, measured so the viewport's visible window always
// fills the available width (timeline = container width minus the left
// title panel). Reported to the viewport, which derives its day-span
// from it, so the chart never leaves dead space and shows more range on
// wider displays.
const scrollEl = ref<HTMLElement | null>(null)
let resizeObserver: ResizeObserver | null = null

function measureViewport(): void {
  if (scrollEl.value) {
    vp.setViewportWidth(scrollEl.value.clientWidth - LEFT_PX)
  }
}

// Frame the project once the content extent is known, then keep the
// timeline width in sync with the container.
onMounted(() => {
  vp.fitToProject()
  measureViewport()
  resizeObserver = new ResizeObserver(measureViewport)
  if (scrollEl.value) resizeObserver.observe(scrollEl.value)
})

onUnmounted(() => {
  resizeObserver?.disconnect()
  resizeObserver = null
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

/** Primary tick row: day (week zoom), week (month zoom, Monday
 *  aligned), or month (quarter zoom). */
const primaryTicks = computed<Tick[]>(() => {
  const out: Tick[] = []
  const start = rangeStart.value
  const end = rangeEnd.value
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
  const end = rangeEnd.value
  if (zoom.value === 'quarter') {
    let d = startOfQuarter(rangeStart.value)
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
    let d = startOfMonth(rangeStart.value)
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

const today = computed(() => startOfDay(new Date()))
const todayInRange = computed(
  () =>
    today.value.getTime() >= rangeStart.value.getTime() &&
    today.value.getTime() <= rangeEnd.value.getTime(),
)
const todayX = computed(() => xOf(today.value))

// ===================== Cycle bands =====================

interface CycleBand {
  key: string
  left: number
  width: number
  label: string
  state: Cycle['state']
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
function cycleStripClass(state: Cycle['state']): string {
  if (state === 'active') return 'bg-accent/15 text-accent border-accent/40'
  if (state === 'planned') return 'bg-surface-hover text-secondary border-subtle'
  return 'bg-surface-alt text-tertiary border-subtle'
}

/** Body shading: a faint wash so the band reads behind the bars
 *  without competing with them. */
function cycleBodyClass(state: Cycle['state']): string {
  return state === 'active' ? 'bg-accent/5' : 'bg-surface-hover/30'
}

// ===================== Bars =====================

interface BarRow {
  card: CardData
  rowIndex: number
  left: number
  width: number
  terminal: boolean
  /** Non-terminal and past its due date: an at-risk (overdue) bar. */
  atRisk: boolean
  /** Clamped right edge / left edge for arrow anchoring. */
  rightX: number
  /** Effective span (with any live reschedule preview applied), so
   *  the resize handle can clamp against the locked start. */
  start: Date
  end: Date
}

// Live reschedule preview: while a bar's due handle is being dragged,
// its end is overridden so the bar resizes under the cursor before the
// write commits on pointerup.
const reschedule = ref<{ cardId: number; start: Date; newEnd: Date } | null>(null)
const bodyEl = ref<HTMLElement | null>(null)

const bars = computed<BarRow[]>(() => {
  const out: BarRow[] = []
  const max = totalWidth.value
  const r = reschedule.value
  for (const it of scheduled.value) {
    const end = r && r.cardId === it.card.id ? r.newEnd : it.end
    const rawLeft = xOf(it.start)
    const rawRight = xOf(end)
    // Drop bars fully outside the window. rowIndex tracks the
    // rendered position (out.length), so dropped cards leave no gap
    // and the absolutely-positioned bars stay aligned with the
    // contiguous title list in the left panel.
    if (rawRight <= 0 || rawLeft >= max) continue
    const left = Math.max(0, rawLeft)
    const right = Math.min(max, rawRight)
    const width = Math.max(pxPerDay.value, right - left)
    const terminal = TERMINAL_CATEGORIES.has(it.card.workflow_state.category)
    out.push({
      card: it.card,
      rowIndex: out.length,
      left,
      width,
      terminal,
      atRisk: !terminal && today.value.getTime() > end.getTime(),
      rightX: right,
      start: it.start,
      end,
    })
  }
  return out
})

// Report the in-window bar count for the toolbar's in-view label.
watchEffect(() => {
  vp.visibleCount.value = bars.value.length
})

/** Right-edge (due) handle drag. Only the due edge moves; the start
 *  is created_at, immutable history, so the left edge is locked. */
function startResize(bar: BarRow, event: PointerEvent): void {
  if (!props.onReschedule) return
  event.stopPropagation()
  reschedule.value = { cardId: bar.card.id, start: bar.start, newEnd: bar.end }
  ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
}
function onResizeMove(event: PointerEvent): void {
  const r = reschedule.value
  if (!r || !bodyEl.value) return
  const rect = bodyEl.value.getBoundingClientRect()
  const days = Math.round((event.clientX - rect.left) / pxPerDay.value)
  let d = addDays(rangeStart.value, days)
  // Never let the due edge cross the locked start.
  if (d.getTime() <= r.start.getTime()) d = addDays(r.start, 1)
  reschedule.value = { ...r, newEnd: d }
}
function endResize(): void {
  const r = reschedule.value
  reschedule.value = null
  // Naive local-midnight datetime (no tz suffix). due_date round-trips
  // through the backend's NaiveDateTime model, whose deserialiser
  // rejects a trailing `Z`; sending the local day also keeps the bar
  // anchored to the day the user dropped it on.
  if (r) props.onReschedule?.(r.cardId, `${format(r.newEnd, 'yyyy-MM-dd')}T00:00:00`)
}

const totalHeight = computed(() => scheduled.value.length * ROW_PX)

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

function barTooltip(b: BarRow): string {
  const it = scheduled.value.find((s) => s.card.id === b.card.id)
  if (!it) return b.card.title
  return `${b.card.title} · ${format(it.start, 'MMM d')} to ${format(it.end, 'MMM d')}`
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
      fromY: src.rowIndex * ROW_PX + ROW_PX / 2,
      toX: dst.left,
      toY: dst.rowIndex * ROW_PX + ROW_PX / 2,
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
  props.onCardClick?.(card.id)
}
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Scroll container. The viewport toolbar (zoom / fit / today /
         pan) lives in the project tab bar, rendered by the route shell
         that owns the shared viewport. -->
    <div ref="scrollEl" class="flex-1 min-h-0 overflow-auto">
      <div class="relative" :style="{ width: `${totalWidth + LEFT_PX}px` }">
        <!-- Sticky left panel: titles + tray -->
        <div
          class="absolute left-0 top-0 z-20 bg-app border-r border-subtle flex flex-col"
          :style="{ width: `${LEFT_PX}px` }"
        >
          <div class="border-b border-subtle bg-surface" style="height: 48px"></div>
          <button
            v-for="row in bars"
            :key="row.card.id"
            type="button"
            class="w-full flex items-center px-3 text-xs text-left text-primary border-b border-subtle/50 hover:bg-surface-hover focus:outline-none focus-visible:bg-surface-hover truncate"
            :style="{ height: `${ROW_PX}px` }"
            :title="row.card.title"
            @click="open(row.card)"
          >
            <span class="font-mono text-tertiary mr-2">#{{ row.card.id }}</span>
            <span class="truncate">{{ row.card.title }}</span>
          </button>

          <!-- Unscheduled tray -->
          <div v-if="unscheduled.length > 0" class="border-t border-subtle mt-auto">
            <button
              type="button"
              class="w-full flex items-center gap-2 px-3 py-2 text-xs font-medium text-secondary hover:bg-surface-hover"
              @click="trayOpen = !trayOpen"
            >
              <span class="text-tertiary transition-transform" :class="trayOpen ? 'rotate-90' : ''">›</span>
              {{ t('gantt-unscheduled', { count: unscheduled.length }) }}
            </button>
            <div v-if="trayOpen" class="max-h-48 overflow-auto">
              <button
                v-for="card in unscheduled"
                :key="card.id"
                type="button"
                class="w-full flex items-center px-3 py-1.5 text-xs text-left text-primary border-t border-subtle/40 hover:bg-surface-hover focus:outline-none focus-visible:bg-surface-hover truncate"
                :title="card.title"
                @click="open(card)"
              >
                <span class="font-mono text-tertiary mr-2">#{{ card.id }}</span>
                <span class="truncate">{{ card.title }}</span>
              </button>
            </div>
          </div>
        </div>

        <!-- Axis + body -->
        <div class="relative" :style="{ marginLeft: `${LEFT_PX}px` }">
          <!-- Secondary band -->
          <div
            class="relative bg-surface border-b border-subtle text-[11px] uppercase tracking-wide font-semibold text-tertiary"
            style="height: 24px"
          >
            <div
              v-for="b in secondaryBands"
              :key="b.key"
              class="absolute top-0 bottom-0 flex items-center px-2 border-r border-subtle/50 overflow-hidden whitespace-nowrap"
              :style="{ left: `${Math.max(0, b.x)}px`, width: `${b.width + Math.min(0, b.x)}px` }"
            >{{ b.label }}</div>
          </div>
          <!-- Primary tick row -->
          <div
            class="relative bg-surface border-b border-subtle text-[11px] tabular-nums text-tertiary"
            style="height: 24px"
          >
            <div
              v-for="tick in primaryTicks"
              :key="tick.key"
              class="absolute top-0 bottom-0 flex items-center pl-1 border-l border-subtle/30 whitespace-nowrap"
              :style="{ left: `${tick.x}px` }"
            >{{ tick.label }}</div>
          </div>

          <!-- Cycle bands strip: one labelled segment per cycle that
               has a date range, aligned to the same scale as the bars. -->
          <div
            v-if="cycleBands.length > 0"
            class="relative bg-app border-b border-subtle"
            style="height: 22px"
          >
            <div
              v-for="band in cycleBands"
              :key="band.key"
              class="absolute top-0 bottom-0 flex items-center px-2 border-l border-r text-[10px] font-medium truncate"
              :class="cycleStripClass(band.state)"
              :style="{ left: `${band.left}px`, width: `${band.width}px` }"
              :title="band.label"
            >{{ band.label }}</div>
          </div>

          <!-- Timeline body -->
          <div
            ref="bodyEl"
            class="relative"
            :style="{ height: `${Math.max(totalHeight, 100)}px`, width: `${totalWidth}px` }"
          >
            <!-- Primary gridlines via repeating gradient keyed to the
                 primary tick step (1 day at week zoom, 7 days otherwise). -->
            <div
              class="absolute inset-0"
              :style="{
                backgroundImage: `repeating-linear-gradient(to right, transparent 0 ${(zoom === 'week' ? 1 : 7) * pxPerDay - 1}px, var(--border-subtle, #e5e7eb33) ${(zoom === 'week' ? 1 : 7) * pxPerDay - 1}px ${(zoom === 'week' ? 1 : 7) * pxPerDay}px)`,
              }"
            ></div>

            <!-- Cycle band shading (behind bars) -->
            <div
              v-for="band in cycleBands"
              :key="`shade-${band.key}`"
              class="absolute top-0 bottom-0 border-l border-r border-dashed border-subtle/40"
              :class="cycleBodyClass(band.state)"
              :style="{ left: `${band.left}px`, width: `${band.width}px` }"
            ></div>

            <!-- Today marker: accent line + a small diamond flag at the
                 top. (A labelled 'Today' pill arrives with the axis pass,
                 which needs an i18n string.) -->
            <div
              v-if="todayInRange"
              class="absolute top-0 bottom-0 w-px bg-accent z-[5] pointer-events-none"
              :style="{ left: `${todayX}px` }"
            >
              <span
                class="absolute -top-px left-1/2 -translate-x-1/2 h-2 w-2 rotate-45 rounded-[1px] bg-accent shadow-sm"
              ></span>
            </div>

            <!-- Arrows -->
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
            <button
              v-for="row in bars"
              :key="row.card.id"
              type="button"
              class="motion-safe:transition-[transform,box-shadow,filter] motion-safe:duration-150 absolute flex items-center rounded-md border overflow-hidden text-left cursor-pointer hover:-translate-y-px hover:shadow-sm hover:brightness-[1.03] focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:z-10"
              :class="[
                barStatusClass(row.card),
                priorityEdgeClass(row.card.priority),
                row.atRisk ? 'ring-1 ring-status-error/60' : '',
                row.terminal ? 'gantt-bar-terminal' : '',
              ]"
              :style="{
                left: `${row.left}px`,
                width: `${Math.max(pxPerDay, row.width - 4)}px`,
                top: `${row.rowIndex * ROW_PX + 4}px`,
                height: `${ROW_PX - 8}px`,
              }"
              :title="barTooltip(row)"
              @click="open(row.card)"
            >
              <!-- Schedule (elapsed-time) fill: a desaturated 'consumed'
                   shade from the bar's start to today. Reads as schedule
                   pressure, not work done. -->
              <span
                v-if="scheduleFillWidth(row) > 0"
                class="absolute inset-y-0 left-0 bg-black/[0.06] dark:bg-white/[0.07] pointer-events-none"
                :style="{ width: `${scheduleFillWidth(row)}px` }"
              ></span>
              <span class="relative z-[1] px-2 text-[11px] text-primary truncate">
                {{ row.card.title }}
              </span>
              <!-- Due-date resize handle (open bars only; created_at is
                   immutable so there's no left handle). -->
              <span
                v-if="onReschedule && !row.terminal"
                class="absolute top-0 bottom-0 right-0 w-2.5 cursor-ew-resize hover:bg-accent/40 z-[2]"
                :title="t('gantt-reschedule-handle')"
                @pointerdown="startResize(row, $event)"
                @pointermove="onResizeMove"
                @pointerup="endResize"
                @click.stop
              ></span>
            </button>
          </div>
        </div>
      </div>
    </div>

    <div
      v-if="bars.length === 0"
      class="px-6 py-4 text-xs text-tertiary italic"
    >
      {{ t('gantt-empty-window') }}
    </div>
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
