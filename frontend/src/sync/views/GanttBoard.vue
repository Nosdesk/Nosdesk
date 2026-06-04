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
import { computed, onMounted, ref } from 'vue'
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
import { TERMINAL_CATEGORIES } from '@/types/workflow'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const props = withDefaults(defineProps<{
  cards: readonly CardData[]
  edges?: readonly DependencyEdge[]
  /** Project cycles, rendered as shaded context bands behind the bars
   *  (only those with both a start and end date). */
  cycles?: readonly Cycle[]
  onCardClick?: (cardId: number) => void
}>(), {
  edges: () => [],
  cycles: () => [],
  onCardClick: undefined,
})

// ===================== Time scale =====================

type Zoom = 'week' | 'month' | 'quarter'
const PX_PER_DAY: Record<Zoom, number> = { week: 26, month: 9, quarter: 3.4 }
const zoom = ref<Zoom>('month')
const pxPerDay = computed(() => PX_PER_DAY[zoom.value])

const DAY_MS = 86_400_000
const ROW_PX = 30
const LEFT_PX = 240

function startOfDay(d: Date): Date {
  const x = new Date(d)
  x.setHours(0, 0, 0, 0)
  return x
}
function daysBetween(a: Date, b: Date): number {
  return Math.round((startOfDay(b).getTime() - startOfDay(a).getTime()) / DAY_MS)
}

const rangeStart = ref<Date>(startOfDay(new Date()))
const rangeEnd = ref<Date>(addDays(startOfDay(new Date()), 45))

function xOf(date: Date): number {
  return daysBetween(rangeStart.value, date) * pxPerDay.value
}
const totalWidth = computed(() => xOf(rangeEnd.value))

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

// ===================== Fit / pan / today =====================

/** Padding around the project span, in days. Wider when zoomed out
 *  so the canvas never crowds the edges. */
function fitPad(): number {
  return Math.max(3, Math.round(7 / (pxPerDay.value / 9)))
}

function fitToProject(): void {
  const items = scheduled.value
  if (items.length === 0) {
    const today = startOfDay(new Date())
    rangeStart.value = addDays(today, -7)
    rangeEnd.value = addDays(today, 45)
    return
  }
  let min = items[0].start
  let max = items[0].end
  for (const it of items) {
    if (it.start.getTime() < min.getTime()) min = it.start
    if (it.end.getTime() > max.getTime()) max = it.end
  }
  const pad = fitPad()
  rangeStart.value = addDays(min, -pad)
  rangeEnd.value = addDays(max, pad)
}

function setZoom(z: Zoom): void {
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

onMounted(fitToProject)

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
  /** Clamped right edge / left edge for arrow anchoring. */
  rightX: number
}

const bars = computed<BarRow[]>(() => {
  const out: BarRow[] = []
  const max = totalWidth.value
  for (const it of scheduled.value) {
    const rawLeft = xOf(it.start)
    const rawRight = xOf(it.end)
    // Drop bars fully outside the window. rowIndex tracks the
    // rendered position (out.length), so dropped cards leave no gap
    // and the absolutely-positioned bars stay aligned with the
    // contiguous title list in the left panel.
    if (rawRight <= 0 || rawLeft >= max) continue
    const left = Math.max(0, rawLeft)
    const right = Math.min(max, rawRight)
    const width = Math.max(pxPerDay.value, right - left)
    out.push({
      card: it.card,
      rowIndex: out.length,
      left,
      width,
      terminal: TERMINAL_CATEGORIES.has(it.card.workflow_state.category),
      rightX: right,
    })
  }
  return out
})

const totalHeight = computed(() => scheduled.value.length * ROW_PX)

function priorityClass(p: CardData['priority']): string {
  if (p === 'urgent' || p === 'high') return 'bg-rose-500/30 border-rose-500/60'
  if (p === 'medium') return 'bg-amber-500/30 border-amber-500/60'
  if (p === 'low') return 'bg-emerald-500/30 border-emerald-500/60'
  return 'bg-surface-hover border-default'
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

const zooms: Zoom[] = ['week', 'month', 'quarter']
const zoomLabel: Record<Zoom, string> = {
  week: 'gantt-zoom-week',
  month: 'gantt-zoom-month',
  quarter: 'gantt-zoom-quarter',
}
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Toolbar -->
    <header class="flex items-center gap-3 px-6 py-3 border-b border-subtle bg-app">
      <h2 class="text-sm font-semibold text-primary">{{ t('gantt-title') }}</h2>

      <!-- Zoom segmented control -->
      <div class="flex items-center rounded-md border border-subtle overflow-hidden">
        <button
          v-for="z in zooms"
          :key="z"
          type="button"
          class="text-xs px-2.5 py-1 transition-colors"
          :class="zoom === z
            ? 'bg-accent text-on-accent font-medium'
            : 'text-secondary hover:bg-surface-hover'"
          @click="setZoom(z)"
        >{{ t(zoomLabel[z]) }}</button>
      </div>

      <button
        type="button"
        class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1 border border-subtle"
        @click="fitToProject"
      >{{ t('gantt-fit') }}</button>
      <button
        type="button"
        class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1 border border-subtle"
        @click="centerOnToday"
      >{{ t('gantt-today') }}</button>

      <div class="flex items-center gap-1">
        <button
          type="button"
          class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1"
          @click="pan(-1)"
        >‹</button>
        <button
          type="button"
          class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1"
          @click="pan(1)"
        >›</button>
      </div>

      <p class="text-[11px] text-tertiary ml-auto">
        {{ t('gantt-tickets-of-total-in-view', { count: cards.length, visible: bars.length }) }}
      </p>
    </header>

    <!-- Scroll container -->
    <div class="flex-1 min-h-0 overflow-auto">
      <div class="relative" :style="{ width: `${totalWidth + LEFT_PX}px` }">
        <!-- Sticky left panel: titles + tray -->
        <div
          class="absolute left-0 top-0 z-20 bg-app border-r border-subtle flex flex-col"
          :style="{ width: `${LEFT_PX}px` }"
        >
          <div class="border-b border-subtle bg-surface" style="height: 48px"></div>
          <div
            v-for="row in bars"
            :key="row.card.id"
            class="flex items-center px-3 text-xs text-primary border-b border-subtle/50 cursor-pointer hover:bg-surface-hover truncate"
            :style="{ height: `${ROW_PX}px` }"
            @click="open(row.card)"
          >
            <span class="font-mono text-tertiary mr-2">#{{ row.card.id }}</span>
            <span class="truncate">{{ row.card.title }}</span>
          </div>

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
              <div
                v-for="card in unscheduled"
                :key="card.id"
                class="flex items-center px-3 py-1.5 text-xs text-primary border-t border-subtle/40 cursor-pointer hover:bg-surface-hover truncate"
                @click="open(card)"
              >
                <span class="font-mono text-tertiary mr-2">#{{ card.id }}</span>
                <span class="truncate">{{ card.title }}</span>
              </div>
            </div>
          </div>
        </div>

        <!-- Axis + body -->
        <div class="relative" :style="{ marginLeft: `${LEFT_PX}px` }">
          <!-- Secondary band -->
          <div
            class="relative bg-surface border-b border-subtle text-[10px] uppercase tracking-wide font-semibold text-tertiary"
            style="height: 24px"
          >
            <div
              v-for="b in secondaryBands"
              :key="b.key"
              class="absolute top-0 bottom-0 flex items-center px-2 border-r border-subtle/50 overflow-hidden whitespace-nowrap"
              :style="{ left: `${b.x}px`, width: `${b.width}px` }"
            >{{ b.label }}</div>
          </div>
          <!-- Primary tick row -->
          <div
            class="relative bg-surface border-b border-subtle text-[10px] tabular-nums text-tertiary"
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

            <!-- Today line -->
            <div
              v-if="todayInRange"
              class="absolute top-0 bottom-0 w-px bg-accent"
              :style="{ left: `${todayX}px` }"
            ></div>

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
            <div
              v-for="row in bars"
              :key="row.card.id"
              class="absolute rounded border cursor-pointer hover:brightness-110 transition-all overflow-hidden"
              :class="[priorityClass(row.card.priority), row.terminal ? 'gantt-bar-terminal' : '']"
              :style="{
                left: `${row.left}px`,
                width: `${Math.max(pxPerDay, row.width - 4)}px`,
                top: `${row.rowIndex * ROW_PX + 4}px`,
                height: `${ROW_PX - 8}px`,
              }"
              :title="barTooltip(row)"
              @click="open(row.card)"
            >
              <span class="px-2 text-[11px] text-primary line-clamp-1 leading-[22px]">
                {{ row.card.title }}
              </span>
            </div>
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
.line-clamp-1 {
  display: -webkit-box;
  -webkit-line-clamp: 1;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

/* Terminal-category bars read as finished: muted, with a subtle
   diagonal stripe overlay. */
.gantt-bar-terminal {
  opacity: 0.55;
  background-image: repeating-linear-gradient(
    45deg,
    transparent 0 5px,
    rgba(0, 0, 0, 0.12) 5px 7px
  );
}
</style>
