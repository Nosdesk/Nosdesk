<!--
Heatmap — tickets created bucketed by weekday × hour. The grid is
7 rows × 24 columns; cell intensity scales with count vs the
maximum cell in the result. Useful for spotting working-hour vs
after-hours load patterns at a glance.

The colour ramp is a single-hue opacity scale on chart-1 (the
first categorical chart token). Accent is reserved for interaction
per the design language; using a chart token here keeps the
heatmap colour-coordinated with the rest of the chart family.
Zero-count cells render at the lightest tint so the grid is
visible even on an empty dashboard. v1 has no tooltip; the
drill-through and hover-to-count behaviour lands with the chart
phases.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import { useTimeRange } from '@/composables/useTimeRange'
import { analyticsService, type HeatmapCell } from '@/services/analyticsService'

const props = defineProps<{
  /** Saved-view uuid for drill-through. v1: the whole heatmap is a
   *  router-link to `/tickets?view=<uuid>` — per-cell routing
   *  (`dow=2&hour=14`) lands in a later wave once the list-side
   *  filter merge knows how to consume it. */
  viewUuid?: string
}>()

const fluent = useFluent()
const t = (k: string) => fluent.$t(k)

const { window: timeWindow } = useTimeRange()

const query = useQuery({
  key: () => ['dashboard', 'heatmap', timeWindow.value.from, timeWindow.value.to],
  query: () =>
    analyticsService.heatmap({
      from: timeWindow.value.from,
      to: timeWindow.value.to,
    }),
})

const cells = computed<HeatmapCell[]>(() => query.data.value?.cells ?? [])
const loading = computed(() => query.status.value === 'pending' && cells.value.length === 0)
const hasError = computed(() => query.status.value === 'error')
const isEmpty = computed(() => !loading.value && !hasError.value && cells.value.length === 0)

const maxValue = computed(() => Math.max(1, ...cells.value.map((c) => c.value)))

const cellLookup = computed<Map<string, number>>(() => {
  const m = new Map<string, number>()
  for (const c of cells.value) {
    m.set(`${c.dow}-${c.hour}`, c.value)
  }
  return m
})

const DOW_LABELS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
const HOUR_LABELS = Array.from({ length: 24 }, (_, h) => h)

function cellValue(dow: number, hour: number): number {
  return cellLookup.value.get(`${dow}-${hour}`) ?? 0
}

function cellAlpha(dow: number, hour: number): number {
  const v = cellValue(dow, hour)
  if (v === 0) return 0.05
  // Logarithmic-ish scale so a single large cell doesn't make
  // every other cell render as nearly transparent.
  return 0.15 + 0.85 * Math.sqrt(v / maxValue.value)
}
</script>

<template>
  <div class="flex flex-col w-full h-full p-3">
    <div v-if="loading" class="flex-1 flex items-center justify-center text-tertiary text-xs">
      {{ t('dashboard-line-chart-loading') }}
    </div>
    <div v-else-if="hasError" class="flex-1 flex items-center justify-center text-status-error text-xs">
      {{ t('dashboard-line-chart-error') }}
    </div>
    <div v-else-if="isEmpty" class="flex-1 flex items-center justify-center text-tertiary text-xs">
      {{ t('dashboard-line-chart-empty') }}
    </div>
    <component
      v-else
      :is="props.viewUuid ? 'router-link' : 'div'"
      :to="props.viewUuid ? { path: '/tickets', query: { view: props.viewUuid } } : undefined"
      :class="[
        'flex flex-col gap-1',
        props.viewUuid ? 'transition-colors hover:bg-surface-hover rounded' : '',
      ]"
    >
      <div class="grid grid-cols-[2rem_repeat(24,minmax(0,1fr))] gap-px text-4xs text-tertiary">
        <span aria-hidden="true" />
        <span
          v-for="h in HOUR_LABELS"
          :key="`h-${h}`"
          class="text-center tabular-nums"
        >
          {{ h % 6 === 0 ? h : '' }}
        </span>
      </div>
      <div
        v-for="(label, dow) in DOW_LABELS"
        :key="`row-${dow}`"
        class="grid grid-cols-[2rem_repeat(24,minmax(0,1fr))] gap-px"
      >
        <span class="text-3xs text-tertiary self-center">{{ label }}</span>
        <span
          v-for="h in HOUR_LABELS"
          :key="`c-${dow}-${h}`"
          class="h-3.5 rounded-sm bg-chart-1 transition-opacity"
          :style="{ opacity: cellAlpha(dow, h) }"
          :title="`${label} ${h}:00 — ${cellValue(dow, h)}`"
        />
      </div>
    </component>
  </div>
</template>
