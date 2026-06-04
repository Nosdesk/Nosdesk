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
import { computed, ref } from 'vue'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import { useTimeRange } from '@/composables/useTimeRange'
import { useElementSize } from '@/composables/useElementSize'
import { useDateStore } from '@/stores/dateStore'
import {
  analyticsService,
  type TsMeasure,
  type TsTimeField,
  type TimeseriesBucket,
} from '@/services/analyticsService'

let gradientUidCounter = 0
function nextGradientUid(): number {
  gradientUidCounter += 1
  return gradientUidCounter
}

const props = withDefaults(
  defineProps<{
    measure: TsMeasure
    timeField: TsTimeField
    /** Saved-view uuid for drill-through. When set, the chart
     *  surface becomes a router-link to `/tickets?view=<uuid>`. */
    viewUuid?: string
  }>(),
  {
    measure: 'count',
    timeField: 'created_at',
  },
)

const fluent = useFluent()
const t = (k: string) => fluent.$t(k)

// Unique gradient id per instance so multiple charts on one dashboard
// don't share (and clobber) the same <linearGradient> definition.
const gradientId = `lc-area-${nextGradientUid()}`

const { window: timeWindow, compare, priorWindow, grain } = useTimeRange()
const dateStore = useDateStore()

// The user's effective timezone. The backend aligns buckets to it and
// the axis labels render in it, so a "today" hourly view lands on the
// user's local hours regardless of the server's or browser's zone.
const tz = computed(() => dateStore.effectiveTimezone)

// The service supports hour | day; week/month grains (power-use
// overrides, never produced by a preset) collapse to day.
const seriesGrain = computed<'hour' | 'day'>(() => (grain.value === 'hour' ? 'hour' : 'day'))

const query = useQuery({
  key: () => [
    'dashboard',
    'timeseries',
    props.measure,
    props.timeField,
    seriesGrain.value,
    tz.value,
    timeWindow.value.from,
    timeWindow.value.to,
  ],
  query: () =>
    analyticsService.timeseries({
      measure: props.measure,
      time_field: props.timeField,
      from: timeWindow.value.from,
      to: timeWindow.value.to,
      grain: seriesGrain.value,
      tz: tz.value,
    }),
})

// Compare-to-prior: fetch the equal-length window immediately before
// the current one, rendered as a faint overlay. The key carries
// `compare` so toggling refetches; when compare is off the query
// resolves to empty buckets without hitting the network.
const priorQuery = useQuery({
  key: () => [
    'dashboard',
    'timeseries-prior',
    props.measure,
    props.timeField,
    seriesGrain.value,
    tz.value,
    priorWindow.value.from,
    priorWindow.value.to,
    compare.value,
  ],
  query: () =>
    compare.value
      ? analyticsService.timeseries({
          measure: props.measure,
          time_field: props.timeField,
          from: priorWindow.value.from,
          to: priorWindow.value.to,
          grain: seriesGrain.value,
          tz: tz.value,
        })
      : Promise.resolve({ buckets: [] }),
})

const buckets = computed<TimeseriesBucket[]>(() => query.data.value?.buckets ?? [])
const priorBuckets = computed<TimeseriesBucket[]>(() =>
  compare.value ? priorQuery.data.value?.buckets ?? [] : [],
)
const loading = computed(() => query.status.value === 'pending' && buckets.value.length === 0)
const hasError = computed(() => query.status.value === 'error')
const isEmpty = computed(() => !loading.value && !hasError.value && buckets.value.every((b) => b.value === 0))

// Real-pixel geometry. The SVG is drawn 1:1 against the measured
// container (viewBox === pixel size) instead of a fixed box stretched
// with preserveAspectRatio="none", which distorted text and strokes.
// `containerRef` is an absolutely-positioned box (see template), so its
// size is purely layout-driven — the SVG inside it can't feed its
// intrinsic aspect ratio back into the container height. useElementSize
// keeps width/height current across every resize.
const containerRef = ref<HTMLElement | null>(null)
const { width, height } = useElementSize(containerRef)

const PAD_LEFT = 32
const PAD_RIGHT = 12
const PAD_TOP = 12
const PAD_BOTTOM = 22
const AXIS_FONT = 11
const innerW = computed(() => Math.max(0, width.value - PAD_LEFT - PAD_RIGHT))
const innerH = computed(() => Math.max(0, height.value - PAD_TOP - PAD_BOTTOM))
const baselineY = computed(() => PAD_TOP + innerH.value)

interface Pt {
  x: number
  y: number
}

const chart = computed(() => {
  const data = buckets.value
  if (data.length === 0 || innerW.value <= 0 || innerH.value <= 0) {
    return { linePath: '', areaPath: '', priorPath: '', last: null as Pt | null, max: 0, yTicks: [] as number[] }
  }
  const prior = priorBuckets.value
  // Scale both series to the same ceiling so the overlay is
  // comparable to the current line (max across both windows).
  const max = Math.max(...data.map((b) => b.value), ...prior.map((b) => b.value), 1)
  // Round the y-axis ceiling to a nice value so ticks are
  // readable. Tens for two-digit ranges, hundreds for three, etc.
  const niceMax = niceCeiling(max)
  const w = innerW.value
  const h = innerH.value
  const base = baselineY.value
  const toPoints = (arr: TimeseriesBucket[]): Pt[] => {
    const step = arr.length > 1 ? w / (arr.length - 1) : 0
    return arr.map((b, i) => ({
      x: PAD_LEFT + i * step,
      y: PAD_TOP + h - (b.value / niceMax) * h,
    }))
  }
  const pts = toPoints(data)
  const linePath = monotonePath(pts)
  // Close the smoothed line down to the baseline for the gradient
  // fill, giving the chart visual body instead of a lone stroke.
  const areaPath =
    pts.length > 0
      ? `${linePath} L${pts[pts.length - 1].x.toFixed(1)},${base.toFixed(1)} L${pts[0].x.toFixed(1)},${base.toFixed(1)} Z`
      : ''
  // Y-axis ticks at 0, max/2, max — three labels so the eye can
  // anchor without crowding.
  const yTicks = [0, niceMax / 2, niceMax]
  return {
    linePath,
    areaPath,
    priorPath: monotonePath(toPoints(prior)),
    last: pts.length > 0 ? pts[pts.length - 1] : null,
    max: niceMax,
    yTicks,
  }
})

function niceCeiling(n: number): number {
  if (n <= 5) return 5
  if (n <= 10) return 10
  const magnitude = Math.pow(10, Math.floor(Math.log10(n)))
  return Math.ceil(n / magnitude) * magnitude
}

/**
 * Monotone cubic Hermite path (Fritsch-Carlson). Produces a smooth
 * curve through the points that never overshoots them — so a
 * flat-then-rising ticket series stays pinned to its values and the
 * area fill can't dip below the baseline (which a plain Catmull-Rom
 * spline would do at sharp transitions). 0 / 1 points degrade to a
 * move / no curve.
 */
function monotonePath(pts: Pt[]): string {
  const n = pts.length
  if (n === 0) return ''
  if (n === 1) return `M${pts[0].x.toFixed(1)},${pts[0].y.toFixed(1)}`

  const dx: number[] = []
  const slope: number[] = []
  for (let i = 0; i < n - 1; i++) {
    const h = pts[i + 1].x - pts[i].x
    dx.push(h)
    slope.push(h === 0 ? 0 : (pts[i + 1].y - pts[i].y) / h)
  }

  const m: number[] = new Array(n)
  m[0] = slope[0]
  m[n - 1] = slope[n - 2]
  for (let i = 1; i < n - 1; i++) {
    m[i] = slope[i - 1] * slope[i] <= 0 ? 0 : (slope[i - 1] + slope[i]) / 2
  }
  // Clamp tangents so each segment stays monotonic (no overshoot).
  for (let i = 0; i < n - 1; i++) {
    if (slope[i] === 0) {
      m[i] = 0
      m[i + 1] = 0
      continue
    }
    const a = m[i] / slope[i]
    const b = m[i + 1] / slope[i]
    const s = a * a + b * b
    if (s > 9) {
      const tau = 3 / Math.sqrt(s)
      m[i] = tau * a * slope[i]
      m[i + 1] = tau * b * slope[i]
    }
  }

  let d = `M${pts[0].x.toFixed(1)},${pts[0].y.toFixed(1)}`
  for (let i = 0; i < n - 1; i++) {
    const h = dx[i]
    const cp1x = pts[i].x + h / 3
    const cp1y = pts[i].y + (m[i] * h) / 3
    const cp2x = pts[i + 1].x - h / 3
    const cp2y = pts[i + 1].y - (m[i + 1] * h) / 3
    d += ` C${cp1x.toFixed(1)},${cp1y.toFixed(1)} ${cp2x.toFixed(1)},${cp2y.toFixed(1)} ${pts[i + 1].x.toFixed(1)},${pts[i + 1].y.toFixed(1)}`
  }
  return d
}

function tickY(value: number): number {
  const max = chart.value.max || 1
  const h = innerH.value
  return PAD_TOP + h - (value / max) * h
}

const xLabels = computed(() => {
  const data = buckets.value
  if (data.length === 0 || innerW.value <= 0)
    return [] as { x: number; label: string; anchor: 'start' | 'middle' | 'end' }[]
  const step = data.length > 1 ? innerW.value / (data.length - 1) : 0
  // Render at most ~6 x-axis labels to avoid overdraw on long
  // windows. Pick evenly-spaced indices including the first and
  // last buckets.
  const target = Math.min(6, data.length)
  const indices: number[] = []
  for (let i = 0; i < target; i += 1) {
    const idx = Math.round(((data.length - 1) * i) / Math.max(target - 1, 1))
    if (!indices.includes(idx)) indices.push(idx)
  }
  return indices.map((i, k) => {
    const bucket = data[i]
    const date = new Date(bucket.ts)
    // Format in the user's effective timezone (matching the backend
    // bucketing) so the labels read in their local time, not the
    // browser's. Hourly buckets show time of day (e.g. "9 AM"); daily
    // and coarser buckets show the date (e.g. "Jun 2").
    const label =
      seriesGrain.value === 'hour'
        ? date.toLocaleTimeString(dateStore.locale, { hour: 'numeric', timeZone: tz.value })
        : date.toLocaleDateString(dateStore.locale, {
            month: 'short',
            day: 'numeric',
            timeZone: tz.value,
          })
    // Anchor the edge labels inward (first start, last end) so they
    // don't clip against the chart edges; interior labels centre.
    const anchor = k === 0 ? 'start' : k === indices.length - 1 ? 'end' : 'middle'
    return {
      x: PAD_LEFT + i * step,
      label,
      anchor,
    }
  })
})
</script>

<template>
  <div ref="containerRef" class="flex flex-col w-full h-full">
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
        'relative block w-full h-full overflow-hidden',
        props.viewUuid ? 'transition-colors hover:bg-surface-hover rounded' : '',
      ]"
    >
    <!-- Absolutely-positioned measured box: its size comes from the
         layout (the widget cell), and the SVG fills it without being
         able to push the container taller via its own aspect ratio. -->
    <div ref="containerRef" class="absolute inset-0">
    <svg
      v-if="width > 0 && height > 0"
      :viewBox="`0 0 ${width} ${height}`"
      class="block w-full h-full"
      role="img"
      :aria-label="t('dashboard-line-chart-aria-label')"
    >
      <!-- Y-axis baseline + ticks. Drawn as faint horizontal rules
           so the line sits over a grid, not floating in a void. -->
      <g class="text-default" stroke="currentColor" stroke-width="1" opacity="0.4">
        <line
          v-for="value in chart.yTicks"
          :key="`gridline-${value}`"
          :x1="PAD_LEFT"
          :x2="width - PAD_RIGHT"
          :y1="tickY(value)"
          :y2="tickY(value)"
        />
      </g>
      <g class="text-tertiary" :font-size="AXIS_FONT">
        <text
          v-for="value in chart.yTicks"
          :key="`ytick-${value}`"
          :x="PAD_LEFT - 6"
          :y="tickY(value) + AXIS_FONT / 3"
          text-anchor="end"
          fill="currentColor"
        >
          {{ Math.round(value) }}
        </text>
      </g>
      <g class="text-tertiary" :font-size="AXIS_FONT">
        <text
          v-for="(item, i) in xLabels"
          :key="`xlabel-${i}`"
          :x="item.x"
          :y="height - 6"
          :text-anchor="item.anchor"
          fill="currentColor"
        >
          {{ item.label }}
        </text>
      </g>

      <!-- Soft accent area fill under the line: gives the chart body so
           a sparse series reads as a filled trend rather than a lone
           diagonal in empty space. Fades to transparent toward the
           baseline. -->
      <defs>
        <linearGradient :id="gradientId" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="var(--color-accent)" stop-opacity="0.22" />
          <stop offset="100%" stop-color="var(--color-accent)" stop-opacity="0" />
        </linearGradient>
      </defs>
      <path v-if="chart.areaPath" :d="chart.areaPath" :fill="`url(#${gradientId})`" stroke="none" />

      <!-- Compare-to-prior overlay: the prior equal-length window as a
           faint dashed line, drawn behind the current line so the
           present data stays the primary signal. -->
      <path
        v-if="chart.priorPath"
        :d="chart.priorPath"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-dasharray="4,3"
        stroke-linejoin="round"
        stroke-linecap="round"
        class="text-tertiary"
        opacity="0.7"
      />
      <!-- The line itself: smoothed monotone curve with round joins so
           transitions read as gentle rather than angular. -->
      <path
        :d="chart.linePath"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linejoin="round"
        stroke-linecap="round"
        class="text-accent"
      />
      <!-- Endpoint marker anchoring the latest value, with a surface
           halo so it reads cleanly over the line + grid. -->
      <circle v-if="chart.last" :cx="chart.last.x" :cy="chart.last.y" r="4" class="text-surface" fill="currentColor" />
      <circle v-if="chart.last" :cx="chart.last.x" :cy="chart.last.y" r="2.5" class="text-accent" fill="currentColor" />
    </svg>
    </div>
    </component>
  </div>
</template>
