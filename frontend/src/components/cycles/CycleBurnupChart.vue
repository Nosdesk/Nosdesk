<script setup lang="ts">
/**
 * Count-based burnup chart for a cycle. Research-grounded redesign (see
 * docs/plans/gantt-cycle-design-overhaul.md):
 *
 * - It's a BURNUP (completed rises toward scope), so scope change reads
 *   as a rising scope line, distinct from progress.
 * - ONE focal series: the accent line is "completed", drawn solid over
 *   the elapsed days and continuing as a dotted FORECAST (projected from
 *   the team's actual throughput) into the remaining days. Scope is the
 *   grey context line, solid to today then a dashed adaptive PACE line
 *   (remaining scope spread over remaining working days). Two hues only
 *   (accent + grey) with past/future shown by solid/dashed, which is
 *   inherently colour-blind-safe. The naive day-zero "ideal" diagonal is
 *   gone.
 * - Lines are DIRECTLY LABELLED at their ends; no legend round-trip.
 * - The shaded band between scope and completed is the outstanding work.
 * - Accessible: role/aria summary, a visually-hidden data table, and a
 *   keyboard-reachable crosshair (focus + arrow keys, Esc to dismiss).
 *
 * The backend emits daily points only up to today (scope + completed);
 * the x-axis is extended by date to the cycle end so the pace/forecast
 * lines have room to run.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import type { BurnupSeries } from '@/services/cyclesService'
import { formatCompactDate } from '@/utils/dateUtils'
import { parseDayMs, buildPaceSeries, buildForecast, type SeriesPoint } from '@/utils/burnupModel'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{ series: BurnupSeries }>()

// Plot geometry. viewBox is 640x200; insets leave room for left axis
// labels, bottom date labels, and right-hand direct line labels.
const VB_W = 640
const VB_H = 200
const left = 34
const right = 62
const top = 10
const bottom = 22
const plotW = VB_W - left - right
const plotH = VB_H - top - bottom

const points = computed(() => props.series.points)
const hasData = computed(() => points.value.length > 0)

// "Today" = the last actual point (backend caps the series at now).
const lastPoint = computed(() => points.value[points.value.length - 1] ?? null)
const completedToday = computed(() => lastPoint.value?.completed ?? 0)
const scopeNow = computed(() => lastPoint.value?.scope ?? props.series.final_scope)

// Date domain spans the whole cycle window so the future (pace/forecast)
// region is visible, even though actual points stop at today.
const domainStartMs = computed(() =>
  hasData.value ? parseDayMs(points.value[0].day) : 0,
)
const domainEndMs = computed(() => {
  const endMs = props.series.end ? parseDayMs(props.series.end) : 0
  const lastMs = lastPoint.value ? parseDayMs(lastPoint.value.day) : 0
  return Math.max(endMs, lastMs, domainStartMs.value + 86_400_000)
})
const todayMs = computed(() => (lastPoint.value ? parseDayMs(lastPoint.value.day) : 0))

// Pace + forecast lines (today -> end). Empty for ended cycles.
const paceSeries = computed<SeriesPoint[]>(() =>
  buildPaceSeries({
    todayMs: todayMs.value,
    endMs: domainEndMs.value,
    completedToday: completedToday.value,
    scope: scopeNow.value,
  }),
)
const forecast = computed(() =>
  buildForecast({
    startMs: domainStartMs.value,
    todayMs: todayMs.value,
    endMs: domainEndMs.value,
    completedToday: completedToday.value,
    scope: scopeNow.value,
  }),
)
const forecastSeries = computed<SeriesPoint[]>(() => forecast.value.series)

const maxY = computed(() => {
  const peaks = [
    props.series.final_scope,
    scopeNow.value,
    ...paceSeries.value.map((p) => p.value),
    ...forecastSeries.value.map((p) => p.value),
    1,
  ]
  return Math.max(...peaks)
})

function xForMs(ms: number): number {
  const span = domainEndMs.value - domainStartMs.value
  return left + (span <= 0 ? 0 : ((ms - domainStartMs.value) / span) * plotW)
}
function xForDay(day: string): number {
  return xForMs(parseDayMs(day))
}
function yFor(v: number): number {
  return top + plotH - (v / maxY.value) * plotH
}

const todayX = computed(() => xForMs(todayMs.value))

function polyFromDayValue(arr: { day: string; value: number }[]): string {
  return arr.map((p) => `${xForDay(p.day)},${yFor(p.value)}`).join(' ')
}
const scopeLine = computed(() =>
  points.value.map((p) => `${xForDay(p.day)},${yFor(p.scope)}`).join(' '),
)
const completedLine = computed(() =>
  points.value.map((p) => `${xForDay(p.day)},${yFor(p.completed)}`).join(' '),
)
const paceLine = computed(() => polyFromDayValue(paceSeries.value))
const forecastLine = computed(() => polyFromDayValue(forecastSeries.value))

// Outstanding-work band: between the scope line (upper) and the
// completed line (lower), over the elapsed days only.
const remainingBand = computed(() => {
  if (!hasData.value) return ''
  const scopePts = points.value.map((p) => `${xForDay(p.day)},${yFor(p.scope)}`)
  const completedPts = points.value
    .map((p) => `${xForDay(p.day)},${yFor(p.completed)}`)
    .reverse()
  return [...scopePts, ...completedPts].join(' ')
})

// Faint horizontal gridlines at quarter fractions of the scale.
const gridYs = computed(() => [0.25, 0.5, 0.75].map((f) => yFor(maxY.value * f)))

const showBaseline = computed(
  () => props.series.start_scope > 0 && props.series.start_scope < props.series.final_scope,
)
const scopeAdded = computed(() => Math.max(0, props.series.final_scope - props.series.start_scope))

const firstDay = computed(() => (hasData.value ? formatCompactDate(points.value[0].day) : ''))
const endDay = computed(() =>
  props.series.end ? formatCompactDate(props.series.end) : '',
)
const hasFuture = computed(() => domainEndMs.value > todayMs.value && paceSeries.value.length > 0)

// Direct end-of-line labels: two only (one per series), each at the
// rightmost point of that series. The grey label tracks scope/pace; the
// accent label tracks completed/forecast. The dashed/dotted style past
// the Today marker is what signals "projection", so the labels name the
// series rather than the segment. Nudged apart when they'd collide (the
// on-track case, where forecast meets pace near scope).
const completedLabelY = computed(() => yFor(completedToday.value))
const scopeLabelY = computed(() => yFor(scopeNow.value))
const labelYs = computed(() => {
  let accent = hasFuture.value && forecastSeries.value.length
    ? yFor(forecastSeries.value[forecastSeries.value.length - 1].value)
    : completedLabelY.value
  let grey = hasFuture.value && paceSeries.value.length
    ? yFor(paceSeries.value[paceSeries.value.length - 1].value)
    : scopeLabelY.value
  const MIN_GAP = 11
  if (Math.abs(accent - grey) < MIN_GAP) {
    const mid = (accent + grey) / 2
    grey = mid - MIN_GAP / 2
    accent = mid + MIN_GAP / 2
  }
  const clamp = (y: number) => Math.min(top + plotH, Math.max(top + 6, y))
  return { accent: clamp(accent), grey: clamp(grey) }
})

// Accessible summary + data table.
const summary = computed(() =>
  t('cycle-burnup-summary', {
    completed: completedToday.value,
    scope: scopeNow.value,
  }),
)

// ---- Hover / focus crosshair + readout -----------------------------
const svgEl = ref<SVGSVGElement | null>(null)
const hoverIndex = ref<number | null>(null)

function indexFromClientX(clientX: number): number | null {
  const svg = svgEl.value
  if (!svg || points.value.length === 0) return null
  const rect = svg.getBoundingClientRect()
  if (rect.width === 0) return null
  const vbX = ((clientX - rect.left) / rect.width) * VB_W
  // Snap to the nearest actual data point by x.
  let best = 0
  let bestDist = Infinity
  points.value.forEach((p, i) => {
    const d = Math.abs(xForDay(p.day) - vbX)
    if (d < bestDist) {
      bestDist = d
      best = i
    }
  })
  return best
}

function onMove(e: PointerEvent): void {
  hoverIndex.value = indexFromClientX(e.clientX)
}
function onLeave(): void {
  hoverIndex.value = null
}
function onFocus(): void {
  if (hoverIndex.value === null && points.value.length) {
    hoverIndex.value = points.value.length - 1
  }
}
function onKeydown(e: KeyboardEvent): void {
  if (points.value.length === 0) return
  if (e.key === 'Escape') {
    hoverIndex.value = null
    return
  }
  if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
    e.preventDefault()
    const cur = hoverIndex.value ?? points.value.length - 1
    const next = e.key === 'ArrowLeft' ? cur - 1 : cur + 1
    hoverIndex.value = Math.min(points.value.length - 1, Math.max(0, next))
  }
}

const hoverPoint = computed(() =>
  hoverIndex.value == null ? null : points.value[hoverIndex.value],
)
const hoverX = computed(() => (hoverPoint.value ? xForDay(hoverPoint.value.day) : 0))
const READOUT_W = 116
const READOUT_H = 46
const readoutX = computed(() =>
  Math.min(VB_W - READOUT_W - 6, Math.max(left, hoverX.value + 8)),
)
const hoverDay = computed(() =>
  hoverPoint.value ? formatCompactDate(hoverPoint.value.day) : '',
)
</script>

<template>
  <div class="flex flex-col gap-2">
    <h4 class="text-xs font-semibold text-secondary uppercase tracking-wide">
      {{ t('cycle-burnup-title') }}
    </h4>

    <p v-if="!hasData" class="text-xs text-tertiary italic">
      {{ t('cycle-burnup-needs-dates') }}
    </p>

    <template v-else>
      <svg
        ref="svgEl"
        :viewBox="`0 0 ${VB_W} ${VB_H}`"
        class="w-full h-auto burnup focus:outline-none focus-visible:ring-2 focus-visible:ring-accent rounded-sm"
        role="img"
        tabindex="0"
        :aria-label="summary"
        @pointermove="onMove"
        @pointerleave="onLeave"
        @focus="onFocus"
        @blur="onLeave"
        @keydown="onKeydown"
      >
        <title>{{ t('cycle-burnup-title') }}</title>
        <desc>{{ summary }}</desc>

        <!-- Axes -->
        <line :x1="left" :y1="top" :x2="left" :y2="top + plotH" class="stroke-subtle" stroke-width="1" />
        <line
          :x1="left"
          :y1="top + plotH"
          :x2="VB_W - right"
          :y2="top + plotH"
          class="stroke-subtle"
          stroke-width="1"
        />

        <!-- Gridlines -->
        <line
          v-for="(gy, i) in gridYs"
          :key="i"
          :x1="left"
          :y1="gy"
          :x2="VB_W - right"
          :y2="gy"
          class="stroke-subtle"
          stroke-width="1"
          stroke-opacity="0.35"
        />

        <!-- Y ticks: 0 and final scope -->
        <text :x="left - 4" :y="yFor(0) + 3" text-anchor="end" class="fill-tertiary text-[10px]">0</text>
        <text
          :x="left - 4"
          :y="yFor(series.final_scope) + 3"
          text-anchor="end"
          class="fill-tertiary text-[10px]"
        >{{ series.final_scope }}</text>

        <!-- Start-scope baseline (creep reference) -->
        <template v-if="showBaseline">
          <line
            :x1="left"
            :y1="yFor(series.start_scope)"
            :x2="VB_W - right"
            :y2="yFor(series.start_scope)"
            class="stroke-subtle"
            stroke-width="1"
            stroke-dasharray="3 3"
          />
          <text
            :x="left - 4"
            :y="yFor(series.start_scope) + 3"
            text-anchor="end"
            class="fill-tertiary text-[10px]"
          >{{ series.start_scope }}</text>
        </template>

        <!-- Outstanding-work band (scope - completed, elapsed days) -->
        <polygon :points="remainingBand" class="fill-secondary burnup-band" />

        <!-- Today marker -->
        <template v-if="hasFuture">
          <line
            :x1="todayX"
            :y1="top"
            :x2="todayX"
            :y2="top + plotH"
            class="stroke-strong"
            stroke-width="1"
            stroke-dasharray="2 3"
            stroke-opacity="0.6"
          />
          <text
            :x="todayX"
            :y="top - 1"
            text-anchor="middle"
            class="fill-tertiary text-[9px] uppercase tracking-wide"
          >{{ t('cycle-burnup-today') }}</text>
        </template>

        <!-- Future: adaptive pace (grey dashed) + forecast (accent dotted) -->
        <polyline
          v-if="paceLine"
          :points="paceLine"
          fill="none"
          class="stroke-tertiary"
          stroke-width="1.5"
          stroke-dasharray="5 4"
        />
        <polyline
          v-if="forecastLine"
          :points="forecastLine"
          fill="none"
          class="stroke-accent"
          stroke-width="1.5"
          stroke-dasharray="1.5 4"
          stroke-linecap="round"
          stroke-opacity="0.85"
        />

        <!-- Elapsed: scope (grey context) + completed (accent focal) -->
        <polyline
          :points="scopeLine"
          fill="none"
          path-length="1"
          class="stroke-secondary burnup-line"
          stroke-width="1.5"
        />
        <polyline
          :points="completedLine"
          fill="none"
          path-length="1"
          class="stroke-accent burnup-line"
          stroke-width="2.5"
        />
        <!-- Latest completed marker -->
        <circle :cx="todayX" :cy="completedLabelY" r="3" class="fill-accent" />

        <!-- Direct end-of-line labels (one per series) -->
        <text
          :x="VB_W - right + 4"
          :y="labelYs.grey + 3"
          class="fill-secondary text-[9px] font-medium"
        >{{ hasFuture ? t('cycle-burnup-label-pace') : t('cycle-burnup-legend-scope') }}</text>
        <text
          :x="VB_W - right + 4"
          :y="labelYs.accent + 3"
          class="fill-accent text-[9px] font-semibold"
        >{{ hasFuture ? t('cycle-burnup-label-forecast') : t('cycle-burnup-legend-completed') }}</text>

        <!-- X date labels -->
        <text :x="left" :y="VB_H - 6" text-anchor="start" class="fill-tertiary text-[10px]">
          {{ firstDay }}
        </text>
        <text :x="VB_W - right" :y="VB_H - 6" text-anchor="end" class="fill-tertiary text-[10px]">
          {{ endDay }}
        </text>

        <!-- Hover / focus crosshair + readout -->
        <template v-if="hoverPoint">
          <line
            :x1="hoverX"
            :y1="top"
            :x2="hoverX"
            :y2="top + plotH"
            class="stroke-strong"
            stroke-width="1"
          />
          <circle :cx="hoverX" :cy="yFor(hoverPoint.scope)" r="3" class="fill-secondary" />
          <circle :cx="hoverX" :cy="yFor(hoverPoint.completed)" r="3.5" class="fill-accent" />
          <g class="burnup-readout">
            <rect
              :x="readoutX"
              :y="top"
              :width="READOUT_W"
              :height="READOUT_H"
              rx="4"
              class="fill-surface stroke-default"
              stroke-width="1"
            />
            <text :x="readoutX + 8" :y="top + 15" class="fill-secondary text-[10px] font-medium">
              {{ hoverDay }}
            </text>
            <text :x="readoutX + 8" :y="top + 28" class="fill-accent text-[10px]">
              {{ t('cycle-burnup-legend-completed') }}: {{ hoverPoint.completed }}
            </text>
            <text :x="readoutX + 8" :y="top + 40" class="fill-secondary text-[10px]">
              {{ t('cycle-burnup-legend-scope') }}: {{ hoverPoint.scope }}
            </text>
          </g>
        </template>
      </svg>

      <!-- Scope-creep callout, promoted from fine print -->
      <p v-if="scopeAdded > 0" class="text-[11px] text-tertiary">
        {{ t('tickets-cycle-scope-added', { count: scopeAdded }) }}
      </p>

      <!-- Visually-hidden data table: full screen-reader + keyboard access. -->
      <table class="sr-only">
        <caption>{{ t('cycle-burnup-table-caption') }}</caption>
        <thead>
          <tr>
            <th scope="col">{{ t('cycle-burnup-col-day') }}</th>
            <th scope="col">{{ t('cycle-burnup-legend-scope') }}</th>
            <th scope="col">{{ t('cycle-burnup-legend-completed') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="p in points" :key="p.day">
            <td>{{ formatCompactDate(p.day) }}</td>
            <td>{{ p.scope }}</td>
            <td>{{ p.completed }}</td>
          </tr>
        </tbody>
      </table>
    </template>
  </div>
</template>

<style scoped>
.burnup-band {
  opacity: 0.07;
}

.burnup-line {
  stroke-dasharray: 1;
  stroke-dashoffset: 1;
  animation: burnup-draw 0.6s ease-out forwards;
}

.burnup-readout {
  pointer-events: none;
}

@keyframes burnup-draw {
  to {
    stroke-dashoffset: 0;
  }
}

@media (prefers-reduced-motion: reduce) {
  .burnup-line {
    animation: none;
    stroke-dashoffset: 0;
  }
}
</style>
