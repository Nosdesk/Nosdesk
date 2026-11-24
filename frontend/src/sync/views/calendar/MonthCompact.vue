<script setup lang="ts">
/**
 * The month where a day is too narrow for a title. Each day is its
 * number and one mark for how much is on it: a short rule that grows
 * with the count, in the overdue color when something on that day is
 * late. Selecting a day shows its jobs under the month.
 */
import { computed, ref } from 'vue'
import type { Day } from '@nosdesk/core/utils/ticketDates'
import {
  isLate,
  jobsOn,
  monthOf,
  weekdayLabel,
  type DayJobs,
  type Month,
  type WallClock,
} from '@nosdesk/core/sync/views/calendar'
import { useDayGrid } from './useDayGrid'

const props = defineProps<{
  month: Month
  weeks: Day[][]
  byDay: ReadonlyMap<Day | null, DayJobs>
  clock: WallClock
  locale: string
  focusDay: Day
  selectedDay: Day
  label: (day: Day) => string
}>()

const emit = defineEmits<{
  (e: 'select', day: Day): void
  (e: 'move', day: Day): void
}>()

/** The count at which the mark is as wide as it gets. */
const FULL = 8

const cells = computed(() =>
  props.weeks.map((week) =>
    week.map((day) => {
      const jobs = jobsOn(props.byDay, day).jobs
      return {
        day,
        number: Number(day.slice(8)),
        inMonth: monthOf(day) === props.month,
        today: day === props.clock.day,
        count: jobs.length,
        late: jobs.some((card) => isLate(card, props.clock)),
      }
    }),
  ),
)

const root = ref<HTMLElement | null>(null)
const keys = useDayGrid({
  root,
  focusDay: () => props.focusDay,
  weeks: () => props.weeks,
  move: (day) => emit('move', day),
  activate: (day) => emit('select', day),
})
</script>

<template>
  <div ref="root" class="flex flex-col px-2" role="grid" data-month-compact>
    <div class="grid grid-cols-7" role="row">
      <div
        v-for="day in weeks[0]"
        :key="day"
        class="py-1 text-center text-2xs leading-4 font-medium text-tertiary"
        role="columnheader"
        :aria-label="weekdayLabel(day, locale, 'long')"
      >{{ weekdayLabel(day, locale, 'narrow') }}</div>
    </div>
    <div v-for="week in cells" :key="week[0].day" class="grid grid-cols-7" role="row">
      <button
        v-for="cell in week"
        :key="cell.day"
        type="button"
        role="gridcell"
        :data-day="cell.day"
        :tabindex="cell.day === focusDay ? 0 : -1"
        :aria-label="label(cell.day)"
        :aria-current="cell.today ? 'date' : undefined"
        :aria-selected="cell.day === selectedDay"
        class="group flex h-12 flex-col items-center gap-1 pt-1 focus:outline-none"
        @click="emit('select', cell.day)"
        @keydown="keys.onKeydown"
      >
        <span
          class="inline-flex h-7 w-7 items-center justify-center rounded-full text-sm tabular-nums transition-colors group-focus-visible:ring-2 group-focus-visible:ring-accent"
          :class="cell.day === selectedDay
            ? cell.today
              ? 'bg-accent font-semibold text-on-accent'
              : 'bg-surface-hover font-semibold text-primary'
            : cell.today
              ? 'font-semibold text-accent'
              : cell.inMonth
                ? 'text-primary'
                : 'text-tertiary'"
        >{{ cell.number }}</span>
        <span
          v-if="cell.count"
          class="h-0.5 rounded-full bg-current"
          :class="cell.late ? 'text-rose-500' : 'text-tertiary'"
          :style="{ width: `${4 + (Math.min(cell.count, FULL) / FULL) * 20}px` }"
          data-load-mark
        />
      </button>
    </div>
  </div>
</template>
