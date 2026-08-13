<script setup lang="ts">
/**
 * Vertical timeline — the gantt, transposed for a phone.
 *
 * Time runs DOWN; concurrent tickets sit side by side as columns. This is the
 * mobile calendar day-view pattern applied to tickets: a ticket with
 * start -> due is structurally an event with start -> end, and everyone
 * already reads that layout without being taught it.
 *
 * Why transpose at all (see docs/plans/gantt-mobile-research.md): a horizontal
 * gantt is bounded by TIME SPAN, and 90 days will never fit in 390px. Vertical
 * is bounded by CONCURRENCY instead — how many tickets are in flight at the
 * same moment, which is a much smaller number — and the axis that is unbounded
 * (time) becomes the one the device scrolls naturally.
 *
 * Three ideas borrowed from the timespace canvas:
 *
 *   1. Proportional flow. Distance down the screen IS distance in time, so a
 *      block's height is its duration. This is what separates a timeline from
 *      an agenda list, where every row is the same height and simultaneity is
 *      invisible.
 *   2. The calendar is a lens, not a container. Civic marks are an ADAPTIVE
 *      measure resolved to whatever the current scale can legibly carry —
 *      days, then weeks, then months — rather than a fixed grid.
 *   3. A fidelity ladder. Content climbs detail as it gets more room:
 *      mark -> titled chip -> full card. This is what stops high concurrency
 *      turning to mush: narrow columns degrade to legible marks instead of
 *      clipped text.
 *
 * Also from timespace: precision is provenance. `spanOf` falls back to
 * `created_at` when a ticket has no authored `start_date`, which is a
 * low-precision guess presented as fact. Inferred starts render hatched so a
 * plan does not masquerade as a measurement.
 *
 * Write path (deliberately narrower than desktop): hold-to-move the block
 * preserves duration. No edge handles — a 36px day marker cannot host one —
 * and no tray-to-canvas drop. One verb, the same `useBarDrag` model, time on Y.
 */
import { computed, nextTick, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { addDays, differenceInCalendarDays, format, startOfDay, startOfMonth, startOfWeek } from 'date-fns'
import type { CardData } from '@nosdesk/core/sync/views/types'
import { splitSchedule, type ScheduledCard } from './rowModel'
import {
  GUTTER,
  assignLanes,
  canvasWidth,
  fidelityFor,
  laneCount as countLanes,
  laneWidth as widthForLanes,
} from './verticalLayout'
import { computeTimelineWindow, landingScrollTop } from './timelineWindow'
import type { GanttCycle } from './types'
import { naiveDay } from './types'
import {
  cycleBodyClass,
  cycleStripClass,
  datedCycleSpans,
  projectCycleBand,
} from './cycleSpans'
import { useBarDrag } from './useBarDrag'
import { daysBetween } from '@/composables/useGanttViewport'
import { TERMINAL_CATEGORIES, coarseStatusBucket } from '@nosdesk/core/types/workflow'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import UserAvatar from '@/components/UserAvatar.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const props = withDefaults(defineProps<{
  cards: readonly CardData[]
  /** Project cycles, rendered as shaded context bands (dated ones only). */
  cycles?: readonly GanttCycle[]
  onCardClick?: (cardId: number) => void
  /**
   * Direct-manipulation write-back for body drag (moves start + due).
   * Omitting the prop keeps the timeline read-only, same contract as
   * the desktop board.
   */
  onReschedule?: (
    cardId: number,
    patch: { start_date?: string; due_date?: string },
  ) => void
}>(), {
  cycles: () => [],
  onCardClick: undefined,
  onReschedule: undefined,
})

/** Vertical scale. 36px/day keeps a fortnight on ~500px, which is one
 *  comfortable scroll, and leaves a single-day block tall enough to letter. */
const PX_PER_DAY = 36
/** Minimum spacing between civic marks before the ruler steps up a unit. Set
 *  below PX_PER_DAY so a day-scale window actually gets DAY marks: at 44 the
 *  ruler stepped straight to weeks and a ten-day window carried two labels,
 *  leaving nothing to read block heights against. A 10px label needs nowhere
 *  near 36px of clearance. */
const MIN_TICK_PX = 30

const rootEl = ref<HTMLElement | null>(null)
const scrollerEl = ref<HTMLElement | null>(null)
const bodyEl = ref<HTMLElement | null>(null)
const width = ref(390)
const viewportHeight = ref(600)

const split = computed(() => splitSchedule(props.cards))

/**
 * Only work that is still in flight gets a bar.
 *
 * `spanOf` gives a TERMINAL ticket `created_at -> closed_at`, which is a record
 * of how long it was open, not a plan. Rendering the two identically conflates
 * a measurement with an intention, and on a phone it is actively destructive: a
 * single cancelled ticket that sat open for six weeks stretches the window to
 * six weeks and pushes everything actually scheduled off-screen. Desktop
 * survives this because you can zoom out; this view has no room to.
 */
const scheduled = computed<ScheduledCard[]>(() =>
  split.value.scheduled
    .filter((s) => !TERMINAL_CATEGORIES.has(s.card.workflow_state.category))
    .map((s) => {
      // Precision is provenance. `spanOf` fills a missing `start_date` from
      // `created_at`, so a ticket raised two months ago and due next week draws
      // as a two-month plan — a guess rendered with the confidence of a fact.
      // Without an authored start we know the DEADLINE, not the span, so it
      // renders as a day-tall marker at the due date instead of a fabricated
      // bar. This is also what makes the window usable: one such ticket
      // previously stretched it to 58 days and pushed everything else away.
      if (s.card.start_date) return s
      return { ...s, start: addDays(s.end, -1) }
    }),
)
const unscheduled = computed(() => split.value.unscheduled)

const cycleSpans = computed(() => datedCycleSpans(props.cycles))

/** Window covering scheduled work and dated cycles, padded and never
 *  shorter than a screenful. Cycles expand the canvas so a framing
 *  cycle is never clipped just because no ticket sits on its edges. */
const window_ = computed(() => {
  const spans = [
    ...scheduled.value.map((s) => ({ start: s.start, end: s.end })),
    ...cycleSpans.value.map((c) => ({ start: c.start, end: c.endExclusive })),
  ]
  return computeTimelineWindow(spans, {
    viewportHeight: viewportHeight.value,
    pxPerDay: PX_PER_DAY,
  })
})

const canvasHeight = computed(() => window_.value.days * PX_PER_DAY)

// ---- drag (move-only; axis Y) ------------------------------------------
// Same model as the desktop board. Handles are intentionally absent: a
// day-tall mark cannot host an edge grip, and duration-preserving move
// is the high-value mobile verb ("this slipped a week").
const pxPerDayRef = computed(() => PX_PER_DAY)
const canvasStartRef = computed(() => window_.value.start)

const barDrag = useBarDrag({
  pxPerDay: pxPerDayRef,
  canvasStart: canvasStartRef,
  bodyEl,
  scroller: scrollerEl,
  axis: 'y',
  onCommit: ({ cardId, start, end }) => {
    // Body move always writes both edges (promotes an inferred start).
    props.onReschedule?.(cardId, {
      start_date: naiveDay(start),
      due_date: naiveDay(end),
    })
  },
})

/** Scheduled items with the live drag preview applied, so the moving
 *  block and its lane reassignment track the finger. */
const scheduledLive = computed<ScheduledCard[]>(() => {
  const p = barDrag.preview.value
  if (!p) return scheduled.value
  return scheduled.value.map((s) =>
    s.card.id === p.cardId ? { ...s, start: p.start, end: p.end } : s,
  )
})

// Lane assignment + the fidelity ladder live in ./verticalLayout as pure
// functions so the concurrency ceiling can be asserted in a unit test rather
// than inferred from a screenshot of whatever the data happens to hold.
const placed = computed(() => assignLanes(scheduledLive.value))
const lanes = computed(() => countLanes(placed.value))
const laneWidth = computed(() => widthForLanes(width.value, lanes.value))
/** Equals the viewport until concurrency pushes columns onto their legibility
 *  floor, past which the canvas is wider and the view pans sideways. */
const canvasW = computed(() => canvasWidth(width.value, lanes.value))

const cycleBands = computed(() =>
  cycleSpans.value.flatMap((span) => {
    const band = projectCycleBand(
      span,
      window_.value.start,
      canvasHeight.value,
      PX_PER_DAY,
      daysBetween,
    )
    return band ? [band] : []
  }),
)

/** Ghost of the pre-drag span so the move reads as a translation.
 *  Lane comes from the pre-drag layout (scheduled, not live) so the
 *  outline stays put while the block reflows under the finger. */
const dragGhost = computed(() => {
  const p = barDrag.preview.value
  if (!p) return null
  const top = daysBetween(window_.value.start, p.origStart) * PX_PER_DAY
  const height = Math.max(22, daysBetween(p.origStart, p.origEnd) * PX_PER_DAY)
  const origLane =
    assignLanes(scheduled.value).find((x) => x.item.card.id === p.cardId)?.lane ?? 0
  // Chip rides the live leading edge so it tracks the drop day.
  const liveLane =
    placed.value.find((x) => x.item.card.id === p.cardId)?.lane ?? origLane
  const colW = Math.max(14, laneWidth.value - 4)
  return {
    top,
    height,
    left: GUTTER + origLane * laneWidth.value,
    width: colW,
    chipLabel: format(p.start, 'MMM d'),
    chipTop: daysBetween(window_.value.start, p.start) * PX_PER_DAY,
    chipLeft: GUTTER + liveLane * laneWidth.value + colW / 2,
  }
})

const canReschedule = computed(() => !!props.onReschedule)
/** Card currently under a live drag preview (template-friendly). */
const draggingCardId = computed(() => barDrag.preview.value?.cardId ?? null)

/** Fidelity for a block, given the room its column and duration give it. */
function fidelity(heightPx: number): 'full' | 'compact' | 'mark' {
  return fidelityFor(laneWidth.value, heightPx)
}

function blockHeight(p: { item: ScheduledCard }): number {
  return Math.max(22, differenceInCalendarDays(p.item.end, p.item.start) * PX_PER_DAY)
}

/** The civic ruler, resolved to the coarsest unit the scale can carry legibly:
 *  days while they are far enough apart, then weeks, then months. */
const ticks = computed(() => {
  const { start, days } = window_.value
  const out: Array<{ y: number; label: string; strong: boolean }> = []
  const dayPx = PX_PER_DAY
  const unit = dayPx >= MIN_TICK_PX ? 'day' : dayPx * 7 >= MIN_TICK_PX ? 'week' : 'month'
  let cursor =
    unit === 'day' ? startOfDay(start)
      : unit === 'week' ? startOfWeek(start, { weekStartsOn: 1 })
        : startOfMonth(start)
  const end = addDays(start, days)
  while (cursor < end) {
    const offset = differenceInCalendarDays(cursor, start)
    if (offset >= 0) {
      out.push({
        y: offset * dayPx,
        label:
          unit === 'day' ? format(cursor, 'EEE d')
            : unit === 'week' ? format(cursor, 'd MMM')
              : format(cursor, 'MMM'),
        strong: unit !== 'day' || cursor.getDay() === 1,
      })
    }
    cursor = unit === 'day' ? addDays(cursor, 1) : unit === 'week' ? addDays(cursor, 7) : startOfMonth(addDays(cursor, 32))
  }
  return out
})

const todayY = computed(() => {
  const offset = differenceInCalendarDays(startOfDay(new Date()), window_.value.start)
  if (offset < 0 || offset > window_.value.days) return null
  return offset * PX_PER_DAY
})

/**
 * Open where the work is.
 *
 * Cycles expand the window, so a project whose cycles began months ago gets a
 * canvas several screens tall with every live ticket at the bottom; opening at
 * the top showed an empty calendar (measured: 6 bars, 0 visible, the first one
 * 2988px down). The desktop board has the same hazard and solves it with
 * `scrollToFirstPopulatedLane`; this is that, with time on Y.
 *
 * Fires on the first render that actually HAS bars, not on mount: the sync pool
 * fills in after mount, so on mount there is nothing to aim at. Runs once — the
 * `landed` latch keeps a later scroll from being yanked back.
 */
function landOnTheWork(): void {
  const el = scrollerEl.value
  if (!el) return
  const tops = placed.value.map((p) => differenceInCalendarDays(p.item.start, window_.value.start) * PX_PER_DAY)
  const bottoms = placed.value.map(
    (p, i) => tops[i] + blockHeight(p),
  )
  el.scrollTop = landingScrollTop({
    // Unclamped on purpose: outside the canvas is how an all-past or
    // all-future plan is detected.
    todayY: differenceInCalendarDays(startOfDay(new Date()), window_.value.start) * PX_PER_DAY,
    firstBarTop: tops.length ? Math.min(...tops) : null,
    lastBarBottom: bottoms.length ? Math.max(...bottoms) : null,
    viewportHeight: el.clientHeight,
    canvasHeight: canvasHeight.value,
  })
}

/**
 * Set the moment the reader takes control, after which nothing moves the
 * canvas under them again.
 *
 * Latching on "we have landed once" is not enough. The sync pool streams bars
 * in and the window is derived from that data, so the first bar to arrive lands
 * on a 684px canvas which then grows to 3276px as the rest follow — a landing
 * that was correct for a canvas which no longer exists, and measurably left the
 * view at scrollTop 0. Re-landing on each change until the reader intervenes is
 * what makes it settle in the right place.
 */
const userTookOver = ref(false)

watch(
  [() => placed.value.length, canvasHeight],
  ([count]) => {
    if (userTookOver.value || count === 0) return
    void nextTick(landOnTheWork)
  },
  { immediate: true },
)

function blockStyle(p: { item: ScheduledCard; lane: number }) {
  const top = differenceInCalendarDays(p.item.start, window_.value.start) * PX_PER_DAY
  const height = blockHeight(p)
  return {
    top: `${top}px`,
    height: `${height}px`,
    left: `${GUTTER + p.lane * laneWidth.value}px`,
    width: `${Math.max(14, laneWidth.value - 4)}px`,
  }
}

/** The block's own date line. A timeline block should say WHEN without making
 *  the reader measure it against the ruler. Deadline-only tickets say "due X";
 *  planned spans say the range. */
function dateLabel(item: ScheduledCard): string {
  if (!item.card.start_date) return t('gantt-due-short', { date: format(item.end, 'd MMM') })
  // Collapse the month when both ends share one: "7 – 13 Aug", not
  // "7 Aug – 13 Aug", which wraps mid-range in an 80px column and reads as a
  // broken string rather than a date.
  const sameMonth = item.start.getMonth() === item.end.getMonth()
    && item.start.getFullYear() === item.end.getFullYear()
  return sameMonth
    ? `${format(item.start, 'd')} – ${format(item.end, 'd MMM')}`
    : `${format(item.start, 'd MMM')} – ${format(item.end, 'd MMM')}`
}

/** Status as a muted fill, matching the desktop bar. Kept separable from
 *  priority so the two signals never merge into one colour. */
function statusClass(card: CardData): string {
  switch (coarseStatusBucket(card.workflow_state.category)) {
    case 'open':
      return 'bg-status-open-muted border-status-open/40'
    case 'in-progress':
      return 'bg-status-in-progress-muted border-status-in-progress/40'
    default:
      return 'bg-status-closed-muted border-status-closed/40'
  }
}

/** Priority as a thin accent on the LEADING edge. The desktop bar puts this on
 *  the left because time runs right; here time runs down, so the leading edge
 *  is the top. Same signal, transposed with the axis. */
function priorityEdgeClass(p: CardData['priority']): string {
  if (p === 'urgent' || p === 'high') return 'border-t-[3px] border-t-priority-high'
  if (p === 'medium') return 'border-t-[3px] border-t-priority-medium'
  if (p === 'low') return 'border-t-[3px] border-t-priority-low'
  return ''
}

/** Weekend bands, so the ruler reads as a calendar at a glance rather than as
 *  anonymous gridlines. */
const weekends = computed(() => {
  const { start, days } = window_.value
  const out: Array<{ y: number; h: number }> = []
  for (let i = 0; i < days; i++) {
    const day = addDays(start, i).getDay()
    if (day === 0 || day === 6) out.push({ y: i * PX_PER_DAY, h: PX_PER_DAY })
  }
  return out
})

/** True when the bar's left edge is a guess (`created_at`) rather than an
 *  authored `start_date`. Rendered hatched: a plan should not read as a fact. */
function inferredStart(card: CardData): boolean {
  return !card.start_date
}

function beginMove(p: { item: ScheduledCard }, event: PointerEvent): void {
  if (!props.onReschedule) return
  // Committed span only — never seed a drag from a live preview residual.
  const base = scheduled.value.find((s) => s.card.id === p.item.card.id) ?? p.item
  barDrag.begin(
    'move',
    { cardId: base.card.id, start: base.start, end: base.end },
    event,
  )
}

function onBlockClick(cardId: number): void {
  if (barDrag.shouldSuppressClick()) return
  props.onCardClick?.(cardId)
}

function onResize(el: HTMLElement | null): void {
  if (!el) return
  rootEl.value = el
  width.value = el.clientWidth
  viewportHeight.value = el.clientHeight
  new ResizeObserver(() => {
    width.value = el.clientWidth
    viewportHeight.value = el.clientHeight
  }).observe(el)
}
</script>

<template>
  <div :ref="(el) => onResize(el as HTMLElement | null)" class="flex flex-col min-h-0 flex-1">
    <!-- Nothing scheduled: say so plainly rather than presenting an empty
         ruler, which reads as a broken view. -->
    <div
      v-if="scheduled.length === 0 && cycleBands.length === 0"
      class="flex-1 min-h-0 flex items-center justify-center px-8 text-center"
    >
      <p class="text-sm text-tertiary max-w-[16rem]">{{ t('gantt-nothing-scheduled') }}</p>
    </div>

    <!-- Scrolls in both axes, but only one of them is ever unbounded: down is
         time, across is concurrency and stays put entirely below five parallel
         tickets. Touch pans a single element diagonally, so no nested scrollers
         compete for the gesture. -->
    <div
      v-else
      ref="scrollerEl"
      class="flex-1 min-h-0 overflow-auto overscroll-contain"
      @wheel.passive="userTookOver = true"
      @touchstart.passive="userTookOver = true"
      @pointerdown="userTookOver = true"
    >
      <!-- Canvas. Height is proportional to the window's duration, so vertical
           distance is time; nothing here is a fixed-height row. Width is the
           viewport unless concurrency has outgrown it. -->
      <!-- pb keeps the last block clear of the tab bar and the home indicator. -->
      <div
        ref="bodyEl"
        class="relative pb-16"
        :style="{ height: `${canvasHeight}px`, width: `${canvasW}px` }"
      >
        <!-- Weekends. Drawn under the measure so the grid reads as a calendar.
             Banding starts after the gutter: the bands belong to the plotting
             area, and keeping the ruler a clean strip is what lets its labels
             stay pinned while the lanes pan under them. -->
        <div
          v-for="w in weekends"
          :key="`w${w.y}`"
          class="absolute right-0 bg-strong/[0.045] pointer-events-none"
          :style="{ top: `${w.y}px`, height: `${w.h}px`, left: `${GUTTER}px` }"
        />

        <!-- Cycle band washes (behind bars, same nesting rule as weekends). -->
        <div
          v-for="band in cycleBands"
          :key="`shade-${band.key}`"
          class="absolute right-0 border-t border-b border-subtle/60 pointer-events-none"
          :class="cycleBodyClass(band.state)"
          :style="{
            top: `${band.offset}px`,
            height: `${band.extent}px`,
            left: `${GUTTER}px`,
          }"
        />

        <!-- Adaptive civic measure. -->
        <div
          v-for="tick in ticks"
          :key="tick.y"
          class="absolute left-0 right-0 flex items-start gap-2 pointer-events-none"
          :style="{ top: `${tick.y}px` }"
        >
          <!-- Pinned: the ruler is the reference for everything on the canvas,
               so panning across concurrency must not take it off-screen. -->
          <span
            class="sticky left-0 z-20 w-11 shrink-0 pl-1 py-0.5 bg-surface text-[10px] tabular-nums leading-none"
            :class="tick.strong ? 'text-secondary' : 'text-tertiary'"
          >{{ tick.label }}</span>
          <span
            class="flex-1 border-t"
            :class="tick.strong ? 'border-default' : 'border-subtle'"
          />
        </div>

        <!-- Cycle labels at the leading edge of each band. Sticky left so
             pan-across-concurrency keeps the name; pointer-events-none so
             they never steal a block's hold-to-drag. -->
        <div
          v-for="band in cycleBands"
          :key="`label-${band.key}`"
          class="absolute z-[5] pointer-events-none"
          :style="{
            top: `${band.offset + 2}px`,
            left: `${GUTTER + 4}px`,
            maxWidth: `${Math.max(48, canvasW - GUTTER - 12)}px`,
          }"
        >
          <span
            class="sticky left-12 inline-block max-w-full truncate rounded px-1.5 py-0.5 text-[10px] font-medium border"
            :class="cycleStripClass(band.state)"
            :title="band.label"
          >{{ band.label }}</span>
        </div>

        <!-- Now. Starts after the gutter so it never strikes through a ruler
             label, and carries a dot so it reads as a marker not a divider. -->
        <div
          v-if="todayY !== null"
          class="absolute right-0 z-10 pointer-events-none flex items-center"
          :style="{ top: `${todayY}px`, left: `${GUTTER - 4}px` }"
        >
          <span class="sticky left-11 h-1.5 w-1.5 rounded-full bg-accent shrink-0" />
          <span class="flex-1 border-t border-accent/60" />
        </div>

        <!-- Ghost of the pre-drag position. -->
        <div
          v-if="dragGhost"
          class="absolute rounded-md border border-dashed border-default/60 bg-surface/40 pointer-events-none z-[15]"
          :style="{
            top: `${dragGhost.top}px`,
            height: `${dragGhost.height}px`,
            left: `${dragGhost.left}px`,
            width: `${dragGhost.width}px`,
          }"
        />

        <!-- Live date chip while moving. -->
        <div
          v-if="dragGhost"
          class="absolute z-30 pointer-events-none rounded-full bg-accent px-1.5 py-px text-[10px] font-semibold text-on-accent tabular-nums whitespace-nowrap"
          :style="{
            top: `${dragGhost.chipTop}px`,
            left: `${dragGhost.chipLeft}px`,
            transform: 'translate(-50%, -50%)',
          }"
        >{{ dragGhost.chipLabel }}</div>

        <!-- Tickets. Height is duration; column is a concurrency lane. -->
        <button
          v-for="p in placed"
          :key="p.item.card.id"
          type="button"
          :data-timeline-card-id="p.item.card.id"
          :data-timeline-has-start-date="p.item.card.start_date ? 'true' : 'false'"
          class="absolute rounded-md border text-left overflow-hidden motion-safe:transition-[box-shadow,filter] motion-safe:duration-150 active:brightness-[0.98] focus:outline-none focus-visible:ring-2 focus-visible:ring-accent z-[12]"
          :class="[
            statusClass(p.item.card),
            priorityEdgeClass(p.item.card.priority),
            inferredStart(p.item.card) ? 'nd-inferred-start' : '',
            canReschedule ? 'cursor-grab active:cursor-grabbing' : '',
            draggingCardId === p.item.card.id ? 'shadow-md ring-1 ring-accent/40' : '',
          ]"
          :style="blockStyle(p)"
          :title="`#${p.item.card.id} ${p.item.card.title}`"
          @pointerdown="beginMove(p, $event)"
          @click="onBlockClick(p.item.card.id)"
        >
          <template v-if="fidelity(blockHeight(p)) === 'full'">
            <div class="px-1.5 py-1 flex flex-col gap-1 h-full">
              <div class="flex items-center gap-1">
                <span class="text-[10px] tabular-nums text-tertiary">#{{ p.item.card.id }}</span>
                <PriorityIndicator
                  v-if="p.item.card.priority !== 'none'"
                  :priority="(p.item.card.priority === 'urgent' ? 'high' : p.item.card.priority) as 'low' | 'medium' | 'high'"
                  size="xs"
                />
              </div>
              <span class="text-[11px] leading-tight text-primary line-clamp-3">{{ p.item.card.title }}</span>
              <span class="text-[10px] text-tertiary tabular-nums whitespace-nowrap">{{ dateLabel(p.item) }}</span>
              <UserAvatar
                v-if="p.item.card.assignee_uuid"
                :uuid="p.item.card.assignee_uuid"
                size="xxs"
                :showName="false"
                :clickable="false"
                class="mt-auto"
              />
            </div>
          </template>

          <template v-else-if="fidelity(blockHeight(p)) === 'compact'">
            <div class="px-1.5 py-1 h-full flex flex-col gap-0.5">
              <span class="text-[10px] tabular-nums text-tertiary">#{{ p.item.card.id }}</span>
              <span class="block text-[11px] leading-tight text-primary line-clamp-4">{{ p.item.card.title }}</span>
              <span class="mt-auto text-[10px] text-tertiary tabular-nums whitespace-nowrap">{{ dateLabel(p.item) }}</span>
            </div>
          </template>

          <!-- Mark. Too small in one axis or both to letter, so it carries the
               id only and stays tappable rather than clipping text mid-word. -->
          <template v-else>
            <span class="flex items-center justify-center w-full h-full bg-accent/15 text-[10px] tabular-nums text-secondary">
              #{{ p.item.card.id }}
            </span>
          </template>
        </button>
      </div>
    </div>

    <!-- Tickets with no due date have no position on a time axis, so they sit
         outside the canvas rather than being given a fabricated one. -->
    <div v-if="unscheduled.length > 0" class="shrink-0 border-t border-default bg-surface-alt">
      <div class="px-3 pt-2 pb-1 text-[11px] uppercase tracking-wide text-tertiary">
        {{ t('gantt-unscheduled', { count: unscheduled.length }) }}
      </div>
      <button
        v-for="card in unscheduled"
        :key="card.id"
        type="button"
        class="w-full flex items-center gap-2 px-3 min-h-[44px] text-left border-t border-subtle active:bg-surface-hover"
        @click="onCardClick?.(card.id)"
      >
        <span class="text-[11px] tabular-nums text-tertiary shrink-0">#{{ card.id }}</span>
        <span class="flex-1 min-w-0 truncate text-[13px] text-primary">{{ card.title }}</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
/* An inferred start is a guess, not a measurement: hatch the leading edge so a
   plan does not read as a fact. */
.nd-inferred-start {
  border-top-style: dashed;
}
</style>
