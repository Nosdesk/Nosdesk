<script setup lang="ts">
/**
 * The month where a day is wide enough to read a title in.
 *
 * Seven equal columns and as many rows as the month has weeks, sharing
 * the height there is. Each column has one line everything in it starts
 * on: the weekday, the day number and every job. The grid is inset by
 * the cell's own padding, so the first column's line is the page's: the
 * month title above it and the tickets header above that.
 *
 * A job is a line of text: its time, if it has one, then its title.
 * Timed jobs come first in time order, then the rest in the day's order.
 * A cell too narrow for both keeps the title. Color means one thing
 * here, a job past its deadline; a finished job steps back. A day holds
 * the lines its height allows and says how many more there are.
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import Icon from '@/components/common/Icon.vue'
import type { Day } from '@nosdesk/core/utils/ticketDates'
import type { CardData } from '@nosdesk/core/sync/views/types'
import { planTime } from '@nosdesk/core/utils/ticketPlan'
import {
  cellLines,
  dayNumber,
  formatCellTime,
  isClosed,
  isLate,
  jobsOn,
  monthOf,
  weekdayLabel,
  type DayJobs,
  type Month,
  type WallClock,
} from '@nosdesk/core/sync/views/calendar'
import type { CalendarOverlay } from './calendarTypes'
import { useDayGrid } from './useDayGrid'

const props = defineProps<{
  month: Month
  weeks: Day[][]
  byDay: ReadonlyMap<Day | null, DayJobs>
  overlaysByDay: ReadonlyMap<Day, CalendarOverlay[]>
  clock: WallClock
  locale: string
  /** Jobs lead with their time and can be carried: off when anchored on
   *  a moment. */
  timed: boolean
  /** The day in the tab order. */
  focusDay: Day
  /** The day whose list is open. */
  activeDay: Day | null
  /** The day a carried job is over, and the job. */
  dropDay: Day | null
  draggedId: number | null
  label: (day: Day) => string
}>()

const emit = defineEmits<{
  (e: 'open', id: number): void
  (e: 'day', day: Day, cell: HTMLElement): void
  (e: 'move', day: Day): void
  (e: 'item-down', id: number, event: PointerEvent): void
}>()

// --- How many lines a day holds ----------------------------------------

// The number row and the job lines, in px: h-7 and leading-5.
const NUMBER_ROW = 28
const LINE = 20

// Under this a cell cannot hold a time and enough of a title to tell
// jobs apart, so the line is all title. The order still puts timed jobs
// first, and the day's list shows the times.
const TIME_FROM = 128

const body = ref<HTMLElement | null>(null)
const lines = ref(4)
const roomy = ref(true)
let observer: ResizeObserver | null = null

function measure(): void {
  const cell = body.value?.querySelector<HTMLElement>('[data-day]')
  if (!cell || !cell.clientHeight) return
  lines.value = Math.max(2, Math.floor((cell.clientHeight - NUMBER_ROW - 4) / LINE))
  roomy.value = cell.clientWidth >= TIME_FROM
}

onMounted(() => {
  measure()
  if (typeof ResizeObserver === 'undefined') return
  observer = new ResizeObserver(measure)
  if (body.value) observer.observe(body.value)
})
onBeforeUnmount(() => observer?.disconnect())

interface Cell {
  day: Day
  number: string
  inMonth: boolean
  today: boolean
  shown: CardData[]
  more: number
  overlays: number
}

const cells = computed<Cell[][]>(() =>
  props.weeks.map((week) =>
    week.map((day) => {
      const { shown, more } = cellLines(jobsOn(props.byDay, day).jobs, lines.value)
      return {
        day,
        number: dayNumber(day, props.month, props.locale),
        inMonth: monthOf(day) === props.month,
        today: day === props.clock.day,
        shown,
        more,
        overlays: props.overlaysByDay.get(day)?.length ?? 0,
      }
    }),
  ),
)

function tone(card: CardData): string {
  if (isLate(card, props.clock)) return 'text-rose-600 dark:text-rose-400'
  if (isClosed(card)) return 'text-tertiary'
  return 'text-primary'
}

function timeOf(card: CardData): string {
  const time = props.timed && roomy.value ? planTime(card) : null
  return time ? formatCellTime(time, props.locale) : ''
}

// --- Keys --------------------------------------------------------------

const keys = useDayGrid({
  root: body,
  focusDay: () => props.focusDay,
  weeks: () => props.weeks,
  move: (day) => emit('move', day),
  activate: (day, cell) => emit('day', day, cell),
})

function onCellClick(day: Day, event: MouseEvent): void {
  if ((event.target as HTMLElement).closest('[data-item]')) return
  emit('move', day)
  emit('day', day, event.currentTarget as HTMLElement)
}

function onMore(day: Day, event: MouseEvent): void {
  const cell = (event.currentTarget as HTMLElement).closest<HTMLElement>('[data-day]')
  if (cell) emit('day', day, cell)
}
</script>

<template>
  <div class="flex min-h-0 min-w-0 flex-1 flex-col px-2" role="grid" :aria-rowcount="weeks.length + 1" data-month-grid>
    <div class="grid grid-cols-7 border-b border-subtle" role="row">
      <div
        v-for="day in weeks[0]"
        :key="day"
        class="px-2 py-1.5 text-xs leading-5 font-medium text-tertiary"
        role="columnheader"
        :aria-label="weekdayLabel(day, locale, 'long')"
      >{{ weekdayLabel(day, locale) }}</div>
    </div>

    <div
      ref="body"
      class="grid min-h-0 flex-1 grid-cols-7 overflow-y-auto"
      style="grid-auto-rows: minmax(6.5rem, 1fr)"
    >
      <div v-for="(week, w) in cells" :key="week[0].day" class="contents" role="row">
        <div
          v-for="cell in week"
          :key="cell.day"
          role="gridcell"
          :data-day="cell.day"
          :data-drop-day="timed ? cell.day : undefined"
          :tabindex="cell.day === focusDay ? 0 : -1"
          :aria-label="label(cell.day)"
          :aria-current="cell.today ? 'date' : undefined"
          :aria-expanded="cell.shown.length + cell.more + cell.overlays > 0 ? activeDay === cell.day : undefined"
          class="flex min-w-0 flex-col overflow-hidden border-r border-subtle px-2 pb-1 transition-colors last:border-r-0 focus:outline-none focus-visible:shadow-[inset_0_0_0_2px_var(--color-accent)]"
          :class="[
            w < cells.length - 1 && 'border-b',
            dropDay === cell.day
              ? 'bg-accent/10 shadow-[inset_0_0_0_2px_var(--color-accent)]'
              : activeDay === cell.day && 'bg-surface-hover/60',
          ]"
          @click="onCellClick(cell.day, $event)"
          @keydown="keys.onKeydown"
        >
          <div class="flex h-7 shrink-0 items-center justify-between gap-1">
            <span
              class="text-xs leading-5 tabular-nums"
              :class="cell.today
                ? '-ml-1 inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-accent px-1 font-semibold text-on-accent'
                : cell.inMonth
                  ? 'font-medium text-secondary'
                  : 'text-tertiary'"
            >{{ cell.number }}</span>
            <span
              v-if="cell.overlays"
              class="flex items-center gap-1 text-2xs text-tertiary tabular-nums"
              aria-hidden="true"
              data-overlay-mark
            ><Icon name="device" size="xs" />{{ cell.overlays }}</span>
          </div>

          <ul class="flex min-w-0 flex-col" role="presentation">
            <li v-for="card in cell.shown" :key="card.id" class="flex min-w-0" role="presentation">
              <button
                type="button"
                tabindex="-1"
                data-item
                :data-id="card.id"
                class="-mx-1 flex min-w-0 items-baseline gap-1 rounded px-1 text-left text-xs leading-5 transition-colors hover:bg-surface-hover"
                :class="[tone(card), timed && 'plan-row-hold', draggedId === card.id && 'opacity-40']"
                :title="card.title"
                @click.stop="emit('open', card.id)"
                @pointerdown="emit('item-down', card.id, $event)"
              >
                <span
                  v-if="timeOf(card)"
                  class="shrink-0 tabular-nums"
                  :class="tone(card) === 'text-primary' && 'text-secondary'"
                >{{ timeOf(card) }}</span>
                <span class="truncate">{{ card.title }}</span>
              </button>
            </li>
            <li v-if="cell.more" class="flex min-w-0" role="presentation">
              <button
                type="button"
                tabindex="-1"
                data-more
                class="-mx-1 rounded px-1 text-left text-xs leading-5 text-secondary transition-colors hover:bg-surface-hover hover:text-primary"
                @click.stop="onMore(cell.day, $event)"
              >{{ $t('calendar-more', { count: cell.more }) }}</button>
            </li>
          </ul>
        </div>
      </div>
    </div>
  </div>
</template>
