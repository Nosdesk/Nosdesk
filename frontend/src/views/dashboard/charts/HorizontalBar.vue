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

/** Label for a bucket key. Priority gets a fixed translation;
 *  category and assignee keys fall back to the raw id for now —
 *  resolving them to human names requires an extra round trip and
 *  ships in Wave 6 alongside drill-through. */
function bucketLabel(b: BreakdownBucket): string {
  if (props.groupBy === 'priority') {
    return t(`dashboard-bar-priority-${b.key}`)
  }
  if (b.key === 'none') return t('dashboard-bar-uncategorised')
  if (b.key === 'unassigned') return t('dashboard-bar-unassigned')
  return b.key
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
      <li v-for="b in buckets" :key="b.key">
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
              class="h-full bg-chart-1 transition-[width] duration-200"
              :style="{ width: `${(b.value / maxValue) * 100}%` }"
            />
          </div>
          <span class="text-tertiary tabular-nums text-right">{{ b.value }}</span>
        </component>
      </li>
    </ul>
  </div>
</template>
