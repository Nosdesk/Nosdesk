<!--
GitHub-style contribution heatmap plot. Fills a flex parent (`flex-1
min-h-0 h-full`) and sizes cells relatively — week columns share
horizontal space (scroll when needed) and each day row shares vertical
space equally. No pixel budget math or ResizeObserver; the dashboard
shell's height chain + optional `#footer` slot own the chrome budget.

Used by TicketHeatmap on the dashboard and anywhere else a 365-day
activity grid is needed inside a bounded card body.
-->
<script setup lang="ts">
import { formatDate } from '@/utils/dateUtils'
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import HeatmapTooltip from '@/components/HeatmapTooltip.vue'

export interface ContributionDay {
  date: string
  count: number
  tickets: { id: number; title: string }[]
}

const props = defineProps<{
  weeks: ContributionDay[][]
  /** Skeleton placeholder while the parent loads ticket data. */
  loading?: boolean
  /** ISO date string for today — days after this render invisible. */
  todayStr: string
}>()

const emit = defineEmits<{
  (e: 'day-click', day: ContributionDay): void
}>()

const fluent = useFluent()

/** Sparse row labels (Mon / Wed / Fri) like GitHub. */
const SPARSE_DAY_LABEL_INDICES = new Set([1, 3, 5])

const DAY_LABEL_KEYS = [
  'ticket-heatmap-day-sun',
  'ticket-heatmap-day-mon',
  'ticket-heatmap-day-tue',
  'ticket-heatmap-day-wed',
  'ticket-heatmap-day-thu',
  'ticket-heatmap-day-fri',
  'ticket-heatmap-day-sat',
] as const

const skeletonWeeks = Array.from({ length: 53 }, () => Array(7).fill(null))

const weekColumns = computed(() =>
  props.loading
    ? skeletonWeeks
    : props.weeks.length > 0
      ? props.weeks
      : skeletonWeeks,
)

function isFutureDate(dateStr: string): boolean {
  return dateStr > props.todayStr
}

function colorClass(count: number): string {
  if (count === 0) return 'heatmap-level-0'
  if (count <= 1) return 'heatmap-level-1'
  if (count <= 2) return 'heatmap-level-2'
  if (count <= 3) return 'heatmap-level-3'
  return 'heatmap-level-4'
}

function tooltipDetails(day: ContributionDay) {
  const formattedDate = formatDate(day.date, 'MMM d, yyyy')
  if (day.count === 0) {
    return {
      title: fluent.$t('ticket-heatmap-tooltip-empty'),
      date: formattedDate,
    }
  }
  return {
    title: fluent.$t('ticket-heatmap-tooltip-count', { count: day.count }),
    date: formattedDate,
    tickets: day.tickets.slice(0, 5),
    totalTickets: day.tickets.length,
  }
}
</script>

<template>
  <div class="flex flex-1 min-h-0 h-full w-full gap-1.5 px-1 py-1">
    <div
      class="grid grid-rows-7 shrink-0 w-7 h-full text-[9px] leading-none text-tertiary tabular-nums"
      aria-hidden="true"
    >
      <span
        v-for="(key, index) in DAY_LABEL_KEYS"
        :key="key"
        class="flex items-center justify-end min-h-0 select-none"
        :class="SPARSE_DAY_LABEL_INDICES.has(index) ? '' : 'invisible'"
      >{{ fluent.$t(key) }}</span>
    </div>

    <div class="flex-1 min-h-0 min-w-0 overflow-x-auto overflow-y-hidden">
      <div class="flex h-full min-h-0" :class="loading ? '' : 'min-w-full'">
        <template v-if="loading">
          <div
            v-for="(week, weekIndex) in weekColumns"
            :key="`sk-${weekIndex}`"
            class="flex flex-1 min-w-[3px] flex-col h-full min-h-0"
          >
            <div
              v-for="dayIndex in 7"
              :key="dayIndex"
              class="heatmap-cell heatmap-level-0 flex-1 min-h-[2px] animate-pulse"
            />
          </div>
        </template>

        <template v-else>
          <div
            v-for="(week, weekIndex) in weekColumns"
            :key="weekIndex"
            class="flex flex-1 min-w-[3px] flex-col h-full min-h-0"
          >
            <HeatmapTooltip
              v-for="(day, dayIndex) in week"
              :key="`${weekIndex}-${dayIndex}`"
              :text="day.count.toString()"
              :details="tooltipDetails(day)"
              :disabled="isFutureDate(day.date)"
              class="relative flex flex-1 min-h-0 min-w-0"
            >
              <div
                class="heatmap-cell w-full h-full min-h-[2px] border"
                :class="[
                  isFutureDate(day.date) ? 'invisible border-transparent' : colorClass(day.count),
                  isFutureDate(day.date) ? '' : 'border-subtle',
                  day.count > 0 && !isFutureDate(day.date)
                    ? 'heatmap-cell--interactive cursor-pointer'
                    : 'cursor-default',
                ]"
                @click="!isFutureDate(day.date) && day.count > 0 && emit('day-click', day)"
              />
            </HeatmapTooltip>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* The hit cells tile edge-to-edge (no flex gap) so the pointer never
   crosses a dead zone between dates and the tooltip tracks seamlessly.
   The visual gap is faked by scaling each swatch down inside its full
   cell, so the gap costs no hit area. */
.heatmap-cell {
  border-radius: 2px;
  transform: scale(0.85);
}

.heatmap-cell--interactive {
  transition:
    transform 140ms ease,
    border-color 140ms ease;
}

/* Hover fills the swatch back out to its full cell (a bounded scale to
   1, never beyond) so it grows into its own gap without overflowing the
   scroll track or reflowing the grid. The highlight reuses the cell's
   own border, recoloured to accent (the app's interactive colour, and
   these cells are clickable), instead of stacking a second ring. */
.heatmap-cell--interactive:hover {
  transform: scale(1);
  z-index: 1;
  border-color: var(--color-accent);
}

.heatmap-level-0 {
  background-color: var(--color-bg-surface-alt);
}

.heatmap-level-1 {
  background-color: color-mix(in srgb, var(--color-status-success) 25%, var(--color-bg-surface-alt));
}

.heatmap-level-2 {
  background-color: color-mix(in srgb, var(--color-status-success) 50%, var(--color-bg-surface-alt));
}

.heatmap-level-3 {
  background-color: color-mix(in srgb, var(--color-status-success) 75%, var(--color-bg-surface-alt));
}

.heatmap-level-4 {
  background-color: var(--color-status-success);
}
</style>
