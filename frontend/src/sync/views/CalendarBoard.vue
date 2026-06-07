<script setup lang="ts">
/**
 * Calendar view, sync-engine version. Renders a month grid and
 * places card pills on the day they're due (or last-active /
 * created — `dateField` chooses).
 *
 * Phase 7 scope:
 * - Month view only. Day / week / agenda land as separate
 *   ViewShape cousins.
 * - Click a card to open its detail.
 * - Cards without a value for `dateField` are surfaced in a
 *   "no date" sidebar so they're not silently invisible.
 *
 * Deferred (architecture spec § 10 phase 7):
 * - RRULE recurring tasks (server-side materialise-on-edit).
 * - Asset overlays (warranty, OS support cutoff,
 *   scheduled maintenance) — needs the device-on-card payload.
 * - Working calendars + business-hours arithmetic.
 * - SLA breach overlay — needs the SLA engine.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import type { CardData } from './types'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import { priorityForBadge } from '@/utils/priorityHelpers'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

type DateField = 'due_date' | 'created_at' | 'last_activity_at'

/** Overlay item rendered on a day cell. The parent fetches these
 * (e.g. device warranty expiries) and passes them in pre-bucketed.
 * The visible-month-changed event lets the parent re-fetch when the
 * user steps the calendar. */
export interface CalendarOverlay {
  /** Stable id for keying. */
  id: string
  /** ISO day string (YYYY-MM-DD); lookup is exact match. */
  date: string
  kind: 'warranty_expiry' | 'maintenance' | 'os_cutoff' | 'sla_breach'
  label: string
  /** Optional click target. The parent decides what "open" means. */
  href?: string
}

const props = withDefaults(defineProps<{
  cards: readonly CardData[]
  /** Which CardData field anchors the card to a day. */
  dateField?: DateField
  /** Day-stamped overlays (device warranty expiries, etc.). */
  overlays?: readonly CalendarOverlay[]
  onCardClick?: (cardId: number) => void
}>(), {
  dateField: 'due_date',
  overlays: () => [],
  onCardClick: undefined,
})

const emit = defineEmits<{
  (e: 'visible-range', range: { start: string; end: string }): void
}>()

// ---------------------------------------------------------------
// Visible month (anchored to the first of the month). Local-only
// state for now; persisting it onto a CalendarViewShape's
// `time_axis` lands when the shape gets a saved-view UI.
// ---------------------------------------------------------------
const cursor = ref<Date>(startOfMonth(new Date()))

function startOfMonth(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), 1)
}

function shiftMonth(delta: number): void {
  cursor.value = new Date(cursor.value.getFullYear(), cursor.value.getMonth() + delta, 1)
}

function goToday(): void {
  cursor.value = startOfMonth(new Date())
}

const monthLabel = computed(() =>
  cursor.value.toLocaleDateString(undefined, { month: 'long', year: 'numeric' }),
)

// ---------------------------------------------------------------
// Grid: six rows of seven days, anchored on the Monday of the
// week containing the first of the month. Always six rows so the
// grid height doesn't reflow as the user steps through months.
// ---------------------------------------------------------------
interface DayCell {
  date: Date
  /** True for days inside `cursor`'s month; false for the leading
   * / trailing trim. */
  inMonth: boolean
  isToday: boolean
}

const grid = computed<DayCell[]>(() => {
  const first = cursor.value
  // Sunday=0 in JS; we want Monday=0 so the grid matches Linear /
  // most ticket tooling.
  const dow = (first.getDay() + 6) % 7
  const start = new Date(first)
  start.setDate(first.getDate() - dow)

  const month = first.getMonth()
  const todayIso = isoDay(new Date())

  const days: DayCell[] = []
  for (let i = 0; i < 42; i++) {
    const d = new Date(start)
    d.setDate(start.getDate() + i)
    days.push({
      date: d,
      inMonth: d.getMonth() === month,
      isToday: isoDay(d) === todayIso,
    })
  }
  return days
})

// ---------------------------------------------------------------
// Bucket cards into ISO day strings so each cell is an O(1)
// lookup. Recomputes when the underlying card list changes.
// ---------------------------------------------------------------
const cardsByDay = computed<Map<string, CardData[]>>(() => {
  const map = new Map<string, CardData[]>()
  for (const card of props.cards) {
    const raw = readDate(card, props.dateField)
    if (!raw) continue
    const key = isoDay(new Date(raw))
    let bucket = map.get(key)
    if (!bucket) {
      bucket = []
      map.set(key, bucket)
    }
    bucket.push(card)
  }
  return map
})

const undatedCards = computed<CardData[]>(() =>
  props.cards.filter((c) => !readDate(c, props.dateField)),
)

const overlaysByDay = computed<Map<string, CalendarOverlay[]>>(() => {
  const map = new Map<string, CalendarOverlay[]>()
  for (const ov of props.overlays) {
    let bucket = map.get(ov.date)
    if (!bucket) {
      bucket = []
      map.set(ov.date, bucket)
    }
    bucket.push(ov)
  }
  return map
})

function overlaysFor(cell: DayCell): CalendarOverlay[] {
  return overlaysByDay.value.get(isoDay(cell.date)) ?? []
}

/** Colour vocabulary per overlay kind. The dot indicators in the
 * date row use the solid `dot` class; the tooltip shows the
 * full text labels on hover so the cell stays readable when
 * tickets fill it. */
const overlayDotClass: Record<CalendarOverlay['kind'], string> = {
  warranty_expiry: 'bg-amber-500',
  maintenance: 'bg-sky-500',
  os_cutoff: 'bg-rose-500',
  sla_breach: 'bg-rose-600',
}

function overlayTooltip(items: CalendarOverlay[]): string {
  return items.map((o) => o.label).join('\n')
}

function readDate(card: CardData, field: DateField): string | null | undefined {
  if (field === 'due_date') return card.due_date
  if (field === 'created_at') return card.created_at
  return card.last_activity_at
}

function isoDay(d: Date): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

function cardsFor(cell: DayCell): CardData[] {
  return cardsByDay.value.get(isoDay(cell.date)) ?? []
}

// ---------------------------------------------------------------
// Mobile agenda. Below md the 7x6 month grid is unreadable, so the
// dated cards render as a chronological list grouped by day (the
// calendar-on-mobile convergence: agenda over grid). Reuses the
// same day buckets as the grid; only days in the visible month that
// actually carry cards appear.
// ---------------------------------------------------------------
const agendaDays = computed<DayCell[]>(() =>
  grid.value.filter((c) => c.inMonth && cardsFor(c).length > 0),
)

function agendaDayLabel(d: Date): string {
  return d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' })
}

function open(cardId: number): void {
  props.onCardClick?.(cardId)
}

const WEEKDAY_LABELS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']

// Reset cursor to the current month if the dateField changes — the
// "useful" view of created_at vs due_date is rarely the same window.
watch(() => props.dateField, () => {
  cursor.value = startOfMonth(new Date())
})

// Emit the visible window whenever the grid changes so the parent
// can refetch overlays. Fires on mount via `immediate: true`.
watch(grid, (cells) => {
  if (cells.length === 0) return
  emit('visible-range', {
    start: isoDay(cells[0].date),
    end: isoDay(cells[cells.length - 1].date),
  })
}, { immediate: true })
</script>

<template>
  <div class="flex h-full flex-col">
    <!-- Toolbar -->
    <header class="flex items-center justify-between px-6 py-3 border-b border-subtle bg-app">
      <div class="flex items-center gap-2">
        <button
          type="button"
          class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1"
          @click="shiftMonth(-1)"
        >‹</button>
        <button
          type="button"
          class="text-xs font-medium text-secondary hover:bg-surface-hover rounded-md px-2 py-1"
          @click="goToday"
        >{{ t('calendar-today') }}</button>
        <button
          type="button"
          class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1"
          @click="shiftMonth(1)"
        >›</button>
        <h2 class="text-sm font-semibold text-primary ml-2 tabular-nums">{{ monthLabel }}</h2>
      </div>
      <div class="flex items-center gap-3">
        <span class="text-[10px] uppercase tracking-wide text-tertiary">{{ t('calendar-anchor-label') }}</span>
        <select
          class="bg-surface border border-subtle rounded-md text-xs px-2 py-1 text-primary"
          :value="dateField"
          disabled
          :title="t('calendar-anchor-tooltip')"
        >
          <option value="due_date">{{ t('calendar-anchor-due-date') }}</option>
          <option value="created_at">{{ t('calendar-anchor-created') }}</option>
          <option value="last_activity_at">{{ t('calendar-anchor-last-activity') }}</option>
        </select>
      </div>
    </header>

    <!-- md+: the month grid + 12rem undated rail. The 7x6 grid only
         earns its space on a wide viewport, so it is hidden below md
         in favour of the agenda list below. -->
    <div class="flex-1 min-h-0 hidden md:grid" style="grid-template-columns: 1fr 12rem">
      <section class="min-h-0 flex flex-col">
        <!-- Weekday header -->
        <div class="grid grid-cols-7 text-[10px] uppercase tracking-wide font-semibold text-tertiary bg-surface border-b border-subtle">
          <div
            v-for="label in WEEKDAY_LABELS"
            :key="label"
            class="px-2 py-1.5 text-center"
          >{{ label }}</div>
        </div>

        <!-- Day cells -->
        <div class="flex-1 grid grid-cols-7 grid-rows-6 min-h-0">
          <div
            v-for="cell in grid"
            :key="cell.date.toISOString()"
            class="border-b border-r border-subtle p-1.5 flex flex-col gap-1 min-w-0 overflow-hidden"
            :class="{
              'bg-surface text-tertiary': !cell.inMonth,
              'bg-app': cell.inMonth,
            }"
          >
            <!-- Date row: number + overlay dots. The dots replace
                 the previous full-width chips; one coloured dot per
                 overlay kind present in this day, with a tooltip
                 listing the underlying labels. Keeps tickets in
                 charge of the cell's vertical space. -->
            <div class="flex items-center justify-between gap-1">
              <span
                class="text-[11px] font-medium tabular-nums"
                :class="{
                  'text-on-accent bg-accent rounded-full inline-flex items-center justify-center w-5 h-5': cell.isToday,
                  'text-secondary': !cell.isToday && cell.inMonth,
                }"
              >{{ cell.date.getDate() }}</span>
              <div
                v-if="overlaysFor(cell).length"
                class="flex items-center gap-0.5 cursor-help"
                :title="overlayTooltip(overlaysFor(cell))"
              >
                <span
                  v-for="ov in overlaysFor(cell).slice(0, 3)"
                  :key="ov.id"
                  class="w-1.5 h-1.5 rounded-full"
                  :class="overlayDotClass[ov.kind]"
                  aria-hidden="true"
                />
                <span
                  v-if="overlaysFor(cell).length > 3"
                  class="text-[9px] text-tertiary tabular-nums"
                >+{{ overlaysFor(cell).length - 3 }}</span>
              </div>
            </div>

            <article
              v-for="card in cardsFor(cell)"
              :key="card.id"
              class="bg-surface rounded border border-default hover:border-strong px-1.5 py-1 cursor-pointer text-[11px] flex items-center gap-1.5 truncate"
              @click="open(card.id)"
            >
              <PriorityIndicator
                v-if="priorityForBadge(card.priority)"
                :priority="priorityForBadge(card.priority)!"
                size="xs"
              />
              <span class="text-primary truncate">{{ card.title }}</span>
            </article>
          </div>
        </div>
      </section>

      <!-- Undated rail -->
      <aside class="border-l border-subtle bg-surface flex flex-col min-h-0">
        <header class="px-3 py-2 border-b border-subtle">
          <h3 class="text-[10px] uppercase tracking-wide font-semibold text-tertiary">
            No date ({{ undatedCards.length }})
          </h3>
        </header>
        <div class="flex-1 overflow-y-auto p-2 flex flex-col gap-1.5">
          <article
            v-for="card in undatedCards"
            :key="card.id"
            class="bg-app rounded border border-default hover:border-strong p-2 cursor-pointer text-xs flex flex-col gap-1"
            @click="open(card.id)"
          >
            <div class="flex items-start gap-1.5">
              <PriorityIndicator
                v-if="priorityForBadge(card.priority)"
                :priority="priorityForBadge(card.priority)!"
                size="xs"
              />
              <span class="text-primary line-clamp-2 flex-1">{{ card.title }}</span>
            </div>
            <span class="text-[10px] text-tertiary">#{{ card.id }}</span>
          </article>
          <p
            v-if="undatedCards.length === 0"
            class="text-[11px] text-tertiary italic text-center mt-4"
          >Every ticket has a date.</p>
        </div>
      </aside>
    </div>

    <!-- Below md: agenda list. Dated cards grouped by day (sticky day
         headers), undated bucket last. Reuses the same day buckets as
         the grid; the whole region scrolls vertically. -->
    <div class="flex-1 min-h-0 md:hidden overflow-y-auto flex flex-col">
      <section v-for="day in agendaDays" :key="day.date.toISOString()" class="flex flex-col">
        <h3
          class="sticky top-0 z-10 bg-app px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wide border-b border-subtle"
          :class="day.isToday ? 'text-accent' : 'text-tertiary'"
        >{{ agendaDayLabel(day.date) }}</h3>
        <div class="flex flex-col gap-1.5 px-3 py-2">
          <article
            v-for="card in cardsFor(day)"
            :key="card.id"
            class="bg-surface rounded border border-default hover:border-strong p-2 cursor-pointer text-sm flex items-center gap-2"
            @click="open(card.id)"
          >
            <PriorityIndicator
              v-if="priorityForBadge(card.priority)"
              :priority="priorityForBadge(card.priority)!"
              size="xs"
            />
            <span class="text-primary truncate flex-1">{{ card.title }}</span>
            <span class="text-[10px] text-tertiary tabular-nums shrink-0">#{{ card.id }}</span>
          </article>
        </div>
      </section>

      <!-- Undated bucket -->
      <section v-if="undatedCards.length > 0" class="flex flex-col">
        <h3 class="sticky top-0 z-10 bg-app px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-tertiary border-b border-subtle">
          No date ({{ undatedCards.length }})
        </h3>
        <div class="flex flex-col gap-1.5 px-3 py-2">
          <article
            v-for="card in undatedCards"
            :key="card.id"
            class="bg-surface rounded border border-default hover:border-strong p-2 cursor-pointer text-sm flex items-center gap-2"
            @click="open(card.id)"
          >
            <PriorityIndicator
              v-if="priorityForBadge(card.priority)"
              :priority="priorityForBadge(card.priority)!"
              size="xs"
            />
            <span class="text-primary truncate flex-1">{{ card.title }}</span>
            <span class="text-[10px] text-tertiary tabular-nums shrink-0">#{{ card.id }}</span>
          </article>
        </div>
      </section>

      <p
        v-if="agendaDays.length === 0 && undatedCards.length === 0"
        class="text-xs text-tertiary italic text-center mt-8 px-4"
      >Nothing scheduled this month.</p>
    </div>
  </div>
</template>

<style scoped>
.line-clamp-2 {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
</style>
