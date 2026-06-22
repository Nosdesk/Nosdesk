<!--
HorizontalBar — categorical breakdown rendered as a stacked list of
labelled rows with a horizontal bar per row. Picked over a vertical
column chart for top-N breakdowns because the rows always need a
text label and horizontal bars give that label its own dedicated
gutter without crowding.

Reads the global time range; viz_config carries group_by + top_n.
Drill-through to the filtered ticket list lands in Wave 6.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import { useTimeRange } from '@/composables/useTimeRange'
import {
  analyticsService,
  type BreakdownBucket,
  type BreakdownGroupBy,
} from '@/services/analyticsService'
import { priorityForBadge, priorityLabel } from '@/utils/priorityHelpers'
import type { Priority } from '@/sync/views/types'

const props = withDefaults(
  defineProps<{
    groupBy: BreakdownGroupBy
    topN?: number
    /** Saved-view uuid to drill into on row click. When set, each
     *  row becomes a router-link to `/tickets?view=<uuid>&segment_key=...&segment_value=...`.
     *  The segment params are advisory: the ticket list applies the
     *  saved view's predicate; the segment params are picked up by
     *  Wave 6+ filter merging on the list side. */
    viewUuid?: string
  }>(),
  {
    topN: 10,
  },
)

const fluent = useFluent()
const t = (k: string) => fluent.$t(k)

const { window: timeWindow } = useTimeRange()

const query = useQuery({
  key: () => [
    'dashboard',
    'breakdown',
    props.groupBy,
    props.topN,
    timeWindow.value.from,
    timeWindow.value.to,
  ],
  query: () =>
    analyticsService.breakdown({
      group_by: props.groupBy,
      from: timeWindow.value.from,
      to: timeWindow.value.to,
      top_n: props.topN,
    }),
})

const buckets = computed<BreakdownBucket[]>(() => query.data.value?.buckets ?? [])
const loading = computed(() => query.status.value === 'pending' && buckets.value.length === 0)
const hasError = computed(() => query.status.value === 'error')
const isEmpty = computed(() => !loading.value && !hasError.value && buckets.value.length === 0)

const maxValue = computed(() => Math.max(1, ...buckets.value.map((b) => b.value)))

/** Label for a bucket key. Priority reuses the canonical priority
 *  labels so every surface reads the same; category and assignee
 *  keys fall back to the raw id for now — resolving them to human
 *  names requires an extra round trip and ships in Wave 6 alongside
 *  drill-through. */
function bucketLabel(b: BreakdownBucket): string {
  if (props.groupBy === 'priority') {
    return priorityLabel(b.key as Priority)
  }
  if (b.key === 'none') return t('dashboard-bar-uncategorised')
  if (b.key === 'unassigned') return t('dashboard-bar-unassigned')
  return b.key
}

/** Okabe-Ito categorical palette, cycled for category/assignee
 *  breakdowns. Spelled out as full class literals so Tailwind's
 *  scanner keeps every utility. See the chart palette note in
 *  main.css. */
const CHART_CLASSES = [
  'bg-chart-1',
  'bg-chart-2',
  'bg-chart-3',
  'bg-chart-4',
  'bg-chart-5',
  'bg-chart-6',
  'bg-chart-7',
  'bg-chart-8',
] as const

/** Bar fill class for a bucket. Priority maps to the semantic
 *  priority palette (urgent collapses to high, none to a neutral
 *  tint) so the bars match the dots used across the app; other
 *  breakdowns cycle the categorical palette by row. */
function barClass(b: BreakdownBucket, index: number): string {
  if (props.groupBy === 'priority') {
    const tier = priorityForBadge(b.key as Priority)
    if (tier === 'low') return 'bg-priority-low'
    if (tier === 'medium') return 'bg-priority-medium'
    if (tier === 'high') return 'bg-priority-high'
    return 'bg-tertiary'
  }
  return CHART_CLASSES[index % CHART_CLASSES.length]
}

function rowLink(b: BreakdownBucket) {
  if (!props.viewUuid) return null
  return {
    path: '/tickets',
    query: {
      view: props.viewUuid,
      segment_key: props.groupBy,
      segment_value: b.key,
    },
  }
}
</script>

<template>
  <div class="flex flex-col w-full h-full p-4">
    <div v-if="loading" class="flex-1 flex items-center justify-center text-tertiary text-xs">
      {{ t('dashboard-line-chart-loading') }}
    </div>
    <div v-else-if="hasError" class="flex-1 flex items-center justify-center text-status-error text-xs">
      {{ t('dashboard-line-chart-error') }}
    </div>
    <div v-else-if="isEmpty" class="flex-1 flex items-center justify-center text-tertiary text-xs">
      {{ t('dashboard-line-chart-empty') }}
    </div>
    <ul v-else class="flex flex-col gap-1.5">
      <li v-for="(b, index) in buckets" :key="b.key">
        <component
          :is="rowLink(b) ? 'router-link' : 'div'"
          :to="rowLink(b) ?? undefined"
          :class="[
            'grid grid-cols-[7rem_1fr_3rem] items-center gap-3 text-xs px-1 py-0.5 rounded',
            rowLink(b) ? 'transition-colors hover:bg-surface-hover' : '',
          ]"
        >
          <span class="text-secondary truncate" :title="bucketLabel(b)">{{ bucketLabel(b) }}</span>
          <div class="h-2 rounded-sm bg-surface-alt overflow-hidden">
            <div
              class="h-full transition-[width] duration-200"
              :class="barClass(b, index)"
              :style="{ width: `${(b.value / maxValue) * 100}%` }"
            />
          </div>
          <span class="text-tertiary tabular-nums text-right">{{ b.value }}</span>
        </component>
      </li>
    </ul>
  </div>
</template>
