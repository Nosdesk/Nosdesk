<script setup lang="ts">
/**
 * Phase 8 GanttViewShape renderer.
 *
 * Layout: a horizontal date axis (column header), one row per
 * visible card, a bar spanning each card's `created_at` to its
 * `due_date` (or to "today" if no due date), and SVG arrows for
 * blocks-type linked_tickets edges.
 *
 * Single-day units only — week / month zoom levels live in the
 * spec's GanttViewShape.zoom_level but the v1 use case (sprint
 * planning, this-quarter overview) is well served by a fixed
 * day axis. The zoom toggle drops in here when needed.
 *
 * The renderer is purely a function of (cards, edges, range);
 * the parent supplies the visible window and listens for click
 * events. Dragging bars to reschedule due_date is a follow-up.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import type { CardData } from './types'
import type { DependencyEdge } from '@/services/dependenciesService'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

// `defineProps` is hoisted out of the setup fn entirely, so the
// default factories can't reference module-level helpers below.
// Inline the start/end computation here.
const props = withDefaults(defineProps<{
  cards: readonly CardData[]
  edges?: readonly DependencyEdge[]
  /** Window the renderer paints. Inclusive of both ends. */
  start?: Date
  end?: Date
  onCardClick?: (cardId: number) => void
}>(), {
  edges: () => [],
  start: () => {
    const d = new Date()
    d.setHours(0, 0, 0, 0)
    return d
  },
  end: () => {
    const d = new Date()
    d.setHours(0, 0, 0, 0)
    d.setDate(d.getDate() + 60)
    return d
  },
  onCardClick: undefined,
})

function startOfDay(d: Date): Date {
  const x = new Date(d)
  x.setHours(0, 0, 0, 0)
  return x
}

const DAY_MS = 86_400_000
const DAY_PX = 28 // grid column width
const ROW_PX = 32

const cursor = ref<Date>(startOfDay(props.start))

function shiftDays(delta: number): void {
  const x = new Date(cursor.value)
  x.setDate(x.getDate() + delta)
  cursor.value = x
}

function goToday(): void {
  cursor.value = startOfDay(new Date())
}

const rangeStart = computed<Date>(() => cursor.value)
const rangeEnd = computed<Date>(() => {
  const x = new Date(cursor.value)
  // 60-day window keeps the SVG cheap (~1700px wide) while still
  // covering "this quarter" for the typical helpdesk planning
  // horizon. Wider needs a zoom toggle.
  x.setDate(x.getDate() + 60)
  return x
})

const dayCount = computed<number>(() => {
  return Math.max(1, Math.round((rangeEnd.value.getTime() - rangeStart.value.getTime()) / DAY_MS))
})

interface DayHeader {
  date: Date
  isMonthBoundary: boolean
  isToday: boolean
}

const days = computed<DayHeader[]>(() => {
  const out: DayHeader[] = []
  const todayKey = isoDay(new Date())
  for (let i = 0; i < dayCount.value; i++) {
    const d = new Date(rangeStart.value)
    d.setDate(d.getDate() + i)
    out.push({
      date: d,
      isMonthBoundary: d.getDate() === 1,
      isToday: isoDay(d) === todayKey,
    })
  }
  return out
})

function isoDay(d: Date): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

interface BarRow {
  card: CardData
  rowIndex: number
  startCol: number
  endCol: number
}

const visibleRows = computed<BarRow[]>(() => {
  const out: BarRow[] = []
  let row = 0
  for (const card of props.cards) {
    const created = card.created_at ? new Date(card.created_at) : null
    const due = card.due_date ? new Date(card.due_date) : new Date()
    if (!created) continue
    const startCol = Math.max(0, Math.floor((startOfDay(created).getTime() - rangeStart.value.getTime()) / DAY_MS))
    const endCol = Math.max(startCol + 1, Math.ceil((startOfDay(due).getTime() - rangeStart.value.getTime()) / DAY_MS) + 1)
    // Skip cards entirely outside the window.
    if (endCol <= 0 || startCol >= dayCount.value) continue
    out.push({
      card,
      rowIndex: row,
      startCol: Math.max(0, startCol),
      endCol: Math.min(dayCount.value, endCol),
    })
    row++
  }
  return out
})

const totalWidth = computed<number>(() => dayCount.value * DAY_PX)
const totalHeight = computed<number>(() => visibleRows.value.length * ROW_PX)

interface Arrow {
  fromX: number
  fromY: number
  toX: number
  toY: number
  key: string
}

const arrows = computed<Arrow[]>(() => {
  const rowByCardId = new Map<number, BarRow>()
  for (const r of visibleRows.value) rowByCardId.set(r.card.id, r)
  const out: Arrow[] = []
  for (const e of props.edges) {
    if (e.relation_type !== 'blocks') continue
    const src = rowByCardId.get(e.from)
    const dst = rowByCardId.get(e.to)
    if (!src || !dst) continue
    const fromX = src.endCol * DAY_PX
    const fromY = src.rowIndex * ROW_PX + ROW_PX / 2
    const toX = dst.startCol * DAY_PX
    const toY = dst.rowIndex * ROW_PX + ROW_PX / 2
    out.push({ fromX, fromY, toX, toY, key: `${e.from}->${e.to}` })
  }
  return out
})

function arrowPath(a: Arrow): string {
  // Cubic bezier: stretch the control points horizontally so the
  // arrow always reads "left to right" even on near-stacked rows.
  const dx = Math.max(20, Math.abs(a.toX - a.fromX) * 0.4)
  return `M${a.fromX},${a.fromY} C${a.fromX + dx},${a.fromY} ${a.toX - dx},${a.toY} ${a.toX},${a.toY}`
}

function fmtDayLabel(d: Date): string {
  return String(d.getDate())
}

function fmtMonthLabel(d: Date): string {
  return d.toLocaleDateString(undefined, { month: 'short', year: 'numeric' })
}

function open(card: CardData): void {
  props.onCardClick?.(card.id)
}

function priorityClass(p: CardData['priority']): string {
  if (p === 'urgent' || p === 'high') return 'bg-rose-500/30 border-rose-500/60'
  if (p === 'medium') return 'bg-amber-500/30 border-amber-500/60'
  if (p === 'low') return 'bg-emerald-500/30 border-emerald-500/60'
  return 'bg-surface-hover border-default'
}

const monthHeaders = computed<{ label: string; col: number; span: number }[]>(() => {
  const out: { label: string; col: number; span: number }[] = []
  let i = 0
  while (i < days.value.length) {
    const d = days.value[i].date
    let span = 1
    while (i + span < days.value.length && !days.value[i + span].isMonthBoundary) {
      span++
    }
    out.push({ label: fmtMonthLabel(d), col: i, span })
    i += span
  }
  return out
})
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Toolbar -->
    <header class="flex items-center justify-between px-6 py-3 border-b border-subtle bg-app">
      <div class="flex items-center gap-2">
        <button
          type="button"
          class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1"
          @click="shiftDays(-14)"
        >‹</button>
        <button
          type="button"
          class="text-xs font-medium text-secondary hover:bg-surface-hover rounded-md px-2 py-1"
          @click="goToday"
        >{{ t('gantt-today') }}</button>
        <button
          type="button"
          class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1"
          @click="shiftDays(14)"
        >›</button>
        <h2 class="text-sm font-semibold text-primary ml-2">{{ t('gantt-title') }}</h2>
      </div>
      <p class="text-[11px] text-tertiary">
        {{ t('gantt-tickets-of-total-in-view', { count: cards.length, visible: visibleRows.length }) }}
      </p>
    </header>

    <!-- Scroll container -->
    <div class="flex-1 min-h-0 overflow-auto">
      <div class="relative" :style="{ width: `${totalWidth + 240}px` }">
        <!-- Sticky left column for ticket titles -->
        <div class="absolute left-0 top-0 z-20 bg-app border-r border-subtle" style="width: 240px">
          <div class="border-b border-subtle bg-surface" style="height: 48px"></div>
          <div
            v-for="row in visibleRows"
            :key="row.card.id"
            class="flex items-center px-3 text-xs text-primary border-b border-subtle/50 cursor-pointer hover:bg-surface-hover truncate"
            :style="{ height: `${ROW_PX}px` }"
            @click="open(row.card)"
          >
            <span class="font-mono text-tertiary mr-2">#{{ row.card.id }}</span>
            <span class="truncate">{{ row.card.title }}</span>
          </div>
        </div>

        <!-- Date axis + grid -->
        <div class="ml-[240px] relative">
          <!-- Month band -->
          <div class="flex bg-surface border-b border-subtle text-[10px] uppercase tracking-wide font-semibold text-tertiary" style="height: 24px">
            <div
              v-for="m in monthHeaders"
              :key="`${m.col}-${m.label}`"
              class="border-r border-subtle/50 flex items-center px-2"
              :style="{ width: `${m.span * DAY_PX}px` }"
            >
              {{ m.label }}
            </div>
          </div>
          <!-- Day band -->
          <div class="flex bg-surface border-b border-subtle text-[10px] tabular-nums" style="height: 24px">
            <div
              v-for="d in days"
              :key="d.date.toISOString()"
              class="border-r border-subtle/30 flex items-center justify-center"
              :class="d.isToday ? 'bg-accent text-on-accent font-semibold' : 'text-tertiary'"
              :style="{ width: `${DAY_PX}px` }"
            >
              {{ fmtDayLabel(d.date) }}
            </div>
          </div>

          <!-- Timeline body: bars + arrows on a single SVG so the
               arrow geometry is exact. Bars are foreignObject-free
               <rect>s with HTML labels overlaid below. -->
          <div class="relative" :style="{ height: `${Math.max(totalHeight, 100)}px`, width: `${totalWidth}px` }">
            <!-- Vertical day grid as a CSS background — cheaper
                 than rendering 60 empty divs. -->
            <div
              class="absolute inset-0"
              :style="{
                backgroundImage: `repeating-linear-gradient(to right, transparent 0 ${DAY_PX - 1}px, var(--border-subtle, #e5e7eb33) ${DAY_PX - 1}px ${DAY_PX}px)`
              }"
            ></div>
            <!-- Today line -->
            <template v-for="d in days" :key="`today-${d.date.toISOString()}`">
              <div
                v-if="d.isToday"
                class="absolute top-0 bottom-0 w-px bg-accent"
                :style="{ left: `${days.indexOf(d) * DAY_PX}px` }"
              ></div>
            </template>

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
              v-for="row in visibleRows"
              :key="row.card.id"
              class="absolute rounded border cursor-pointer hover:brightness-110 transition-all overflow-hidden"
              :class="priorityClass(row.card.priority)"
              :style="{
                left: `${row.startCol * DAY_PX}px`,
                width: `${(row.endCol - row.startCol) * DAY_PX - 4}px`,
                top: `${row.rowIndex * ROW_PX + 4}px`,
                height: `${ROW_PX - 8}px`,
              }"
              :title="row.card.title"
              @click="open(row.card)"
            >
              <span class="px-2 text-[11px] text-primary line-clamp-1 leading-[24px]">
                {{ row.card.title }}
              </span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div
      v-if="visibleRows.length === 0"
      class="px-6 py-4 text-xs text-tertiary italic"
    >
      No tickets fall inside this window. Use the arrows or "Today" to step the timeline.
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
</style>
