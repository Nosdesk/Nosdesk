<!--
LineChart — a single-measure daily time-series rendered as an SVG
polyline. No charting library; the data shape is small (≤365 daily
buckets even for the longest preset), the visual vocabulary is one
line + a baseline + axis ticks, and pulling in a charting dep
would dwarf the actual chart code.

Reads the global time range (useTimeRange) so it re-fetches when
the time-range chip cluster changes. viz_config carries the
measure + time_field + grain (day-only for v1). Drill-through to
the filtered ticket list lands in Wave 6.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import { useTimeRange } from '@/composables/useTimeRange'
import {
  analyticsService,
  type TsMeasure,
  type TsTimeField,
  type TimeseriesBucket,
} from '@/services/analyticsService'

const props = withDefaults(
  defineProps<{
    measure: TsMeasure
    timeField: TsTimeField
  }>(),
  {
    measure: 'count',
    timeField: 'created_at',
  },
)

const fluent = useFluent()
const t = (k: string) => fluent.$t(k)

const { window: timeWindow } = useTimeRange()

const query = useQuery({
  key: () => [
    'dashboard',
    'timeseries',
    props.measure,
    props.timeField,
    timeWindow.value.from,
    timeWindow.value.to,
  ],
  query: () =>
    analyticsService.timeseries({
      measure: props.measure,
      time_field: props.timeField,
      from: timeWindow.value.from,
      to: timeWindow.value.to,
    }),
})

const buckets = computed<TimeseriesBucket[]>(() => query.data.value?.buckets ?? [])
const loading = computed(() => query.status.value === 'pending' && buckets.value.length === 0)
const hasError = computed(() => query.status.value === 'error')
const isEmpty = computed(() => !loading.value && !hasError.value && buckets.value.every((b) => b.value === 0))

const VIEWBOX_W = 320
const VIEWBOX_H = 80
const PAD_LEFT = 24
const PAD_RIGHT = 4
const PAD_TOP = 4
const PAD_BOTTOM = 14

const chart = computed(() => {
  const data = buckets.value
  if (data.length === 0) {
    return { path: '', max: 0, yTicks: [] as number[] }
  }
  const max = Math.max(...data.map((b) => b.value), 1)
  // Round the y-axis ceiling to a nice value so ticks are
  // readable. Tens for two-digit ranges, hundreds for three, etc.
  const niceMax = niceCeiling(max)
  const innerW = VIEWBOX_W - PAD_LEFT - PAD_RIGHT
  const innerH = VIEWBOX_H - PAD_TOP - PAD_BOTTOM
  const step = data.length > 1 ? innerW / (data.length - 1) : 0
  const path = data
    .map((b, i) => {
      const x = PAD_LEFT + i * step
      const y = PAD_TOP + innerH - (b.value / niceMax) * innerH
      return `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`
    })
    .join(' ')
  // Y-axis ticks at 0, max/2, max — three labels so the eye can
  // anchor without crowding.
  const yTicks = [0, niceMax / 2, niceMax]
  return { path, max: niceMax, yTicks }
})

function niceCeiling(n: number): number {
  if (n <= 5) return 5
  if (n <= 10) return 10
  const magnitude = Math.pow(10, Math.floor(Math.log10(n)))
  return Math.ceil(n / magnitude) * magnitude
}

function tickY(value: number): number {
  const max = chart.value.max || 1
  const innerH = VIEWBOX_H - PAD_TOP - PAD_BOTTOM
  return PAD_TOP + innerH - (value / max) * innerH
}

const xLabels = computed(() => {
  const data = buckets.value
  if (data.length === 0) return [] as { x: number; label: string }[]
  const innerW = VIEWBOX_W - PAD_LEFT - PAD_RIGHT
  const step = data.length > 1 ? innerW / (data.length - 1) : 0
  // Render at most ~6 x-axis labels to avoid overdraw on long
  // windows. Pick evenly-spaced indices including the first and
  // last buckets.
  const target = Math.min(6, data.length)
  const indices: number[] = []
  for (let i = 0; i < target; i += 1) {
    const idx = Math.round(((data.length - 1) * i) / Math.max(target - 1, 1))
    if (!indices.includes(idx)) indices.push(idx)
  }
  return indices.map((i) => {
    const bucket = data[i]
    const date = new Date(bucket.ts)
    const month = date.toLocaleDateString(undefined, { month: 'short' })
    const day = date.getDate()
    return {
      x: PAD_LEFT + i * step,
      label: `${month} ${day}`,
    }
  })
})
</script>

<template>
  <div class="flex flex-col w-full h-full">
    <div v-if="loading" class="flex-1 flex items-center justify-center text-tertiary text-xs">
      {{ t('dashboard-line-chart-loading') }}
    </div>
    <div v-else-if="hasError" class="flex-1 flex items-center justify-center text-status-error text-xs">
      {{ t('dashboard-line-chart-error') }}
    </div>
    <div v-else-if="isEmpty" class="flex-1 flex items-center justify-center text-tertiary text-xs">
      {{ t('dashboard-line-chart-empty') }}
    </div>
    <svg
      v-else
      :viewBox="`0 0 ${VIEWBOX_W} ${VIEWBOX_H}`"
      preserveAspectRatio="none"
      class="w-full h-full"
      role="img"
      :aria-label="t('dashboard-line-chart-aria-label')"
    >
      <!-- Y-axis baseline + ticks. Drawn as faint horizontal rules
           so the line sits over a grid, not floating in a void. -->
      <g class="text-default" stroke="currentColor" stroke-width="0.5" opacity="0.4">
        <line
          v-for="value in chart.yTicks"
          :key="`gridline-${value}`"
          :x1="PAD_LEFT"
          :x2="VIEWBOX_W - PAD_RIGHT"
          :y1="tickY(value)"
          :y2="tickY(value)"
        />
      </g>
      <g class="text-tertiary" font-size="6">
        <text
          v-for="value in chart.yTicks"
          :key="`ytick-${value}`"
          :x="PAD_LEFT - 4"
          :y="tickY(value) + 2"
          text-anchor="end"
          fill="currentColor"
        >
          {{ Math.round(value) }}
        </text>
      </g>
      <g class="text-tertiary" font-size="6">
        <text
          v-for="(item, i) in xLabels"
          :key="`xlabel-${i}`"
          :x="item.x"
          :y="VIEWBOX_H - 2"
          text-anchor="middle"
          fill="currentColor"
        >
          {{ item.label }}
        </text>
      </g>
      <!-- The line itself. Pure stroke; no fill area underneath so
           the chart reads as a single quantity rather than a stack. -->
      <path :d="chart.path" fill="none" stroke="currentColor" stroke-width="1.5" class="text-accent" />
    </svg>
  </div>
</template>
