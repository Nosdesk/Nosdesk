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
 *   grey context line, solid to today then a dashed adaptive PACE line.
 *   Two hues only (accent + grey), past/future shown by solid/dashed.
 * - Lines are DIRECTLY LABELLED at their ends; no legend round-trip.
 * - The shaded band between scope and completed is the outstanding work.
 *
 * Responsiveness is declarative, not micromanaged. The frame is a fixed
 * responsive HEIGHT that fills the available WIDTH; the SVG stretches to
 * it with `preserveAspectRatio="none"` and every stroke uses
 * `vector-effect: non-scaling-stroke`, so lines stay crisp at any width
 * with no JS measurement. Geometry (lines, band, gridlines) lives in the
 * SVG; all TEXT and point-markers live in an HTML overlay positioned by
 * percentage, so type stays a fixed legible size and dots never distort
 * into ellipses. The browser does the layout; we just declare intent.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import type { BurnupSeries } from '@nosdesk/core/services/cyclesService'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'
import { parseDayMs, buildPaceSeries, buildForecast, type SeriesPoint } from '@nosdesk/core/utils/burnupModel'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{ series: BurnupSeries }>()

// Abstract coordinate space. preserveAspectRatio="none" maps it linearly
// onto the frame, so these units are arbitrary; insets just reserve room
// for the overlaid axis labels.
const VB_W = 640
const VB_H = 200
const left = 30
const right = 30
const top = 12
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
const domainStartMs = computed(() => (hasData.value ? parseDayMs(points.value[0].day) : 0))
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

// Coordinate-space -> frame percentage. Exact because the SVG stretches
// linearly (preserveAspectRatio="none"), so the HTML overlay lands on the
// same points as the SVG geometry at any size.
function xPct(vx: number): number {
  return (vx / VB_W) * 100
}
function yPct(vy: number): number {
  return (vy / VB_H) * 100
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

// Outstanding-work band: between the scope line (upper) and the completed
// line (lower), over the elapsed days only.
const remainingBand = computed(() => {
  if (!hasData.value) return ''
  const scopePts = points.value.map((p) => `${xForDay(p.day)},${yFor(p.scope)}`)
  const completedPts = points.value.map((p) => `${xForDay(p.day)},${yFor(p.completed)}`).reverse()
  return [...scopePts, ...completedPts].join(' ')
})

// Faint horizontal gridlines at quarter fractions of the scale.
const gridYs = computed(() => [0.25, 0.5, 0.75].map((f) => yFor(maxY.value * f)))

const showBaseline = computed(
  () => props.series.start_scope > 0 && props.series.start_scope < props.series.final_scope,
)
const scopeAdded = computed(() => Math.max(0, props.series.final_scope - props.series.start_scope))

const firstDay = computed(() => (hasData.value ? formatCompactDate(points.value[0].day) : ''))
const endDay = computed(() => (props.series.end ? formatCompactDate(props.series.end) : ''))
const hasFuture = computed(() => domainEndMs.value > todayMs.value && paceSeries.value.length > 0)

// Direct end-of-line labels: two only (one per series), at the rightmost
// point of that series, nudged apart when they'd collide (the on-track
// case, where forecast meets pace near scope).
const completedLabelY = computed(() => yFor(completedToday.value))
const scopeLabelY = computed(() => yFor(scopeNow.value))
const labelYs = computed(() => {
  let accent =
    hasFuture.value && forecastSeries.value.length
      ? yFor(forecastSeries.value[forecastSeries.value.length - 1].value)
      : completedLabelY.value
  let grey =
    hasFuture.value && paceSeries.value.length
      ? yFor(paceSeries.value[paceSeries.value.length - 1].value)
      : scopeLabelY.value
  // In viewBox-Y units (0-200), which map to ~0.9px each at the frame's
  // height. The labels are ~10px tall, so this keeps a clear gap between
  // them when the two series end close together.
  const MIN_GAP = 22
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
  t('cycle-burnup-summary', { completed: completedToday.value, scope: scopeNow.value }),
)

// ---- Hover / focus crosshair + readout -----------------------------
const frameEl = ref<HTMLElement | null>(null)
const hoverIndex = ref<number | null>(null)

function indexFromClientX(clientX: number): number | null {
  const frame = frameEl.value
  if (!frame || points.value.length === 0) return null
  const rect = frame.getBoundingClientRect()
  if (rect.width === 0) return null
  const vbX = ((clientX - rect.left) / rect.width) * VB_W
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
  if (hoverIndex.value === null && points.value.length) hoverIndex.value = points.value.length - 1
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

const hoverPoint = computed(() => (hoverIndex.value == null ? null : points.value[hoverIndex.value]))
const hoverX = computed(() => (hoverPoint.value ? xForDay(hoverPoint.value.day) : 0))
const hoverDay = computed(() => (hoverPoint.value ? formatCompactDate(hoverPoint.value.day) : ''))
// Flip the readout to the left of the crosshair once it's past 55% so it
// never spills off the right edge.
const readoutLeft = computed(() => xPct(hoverX.value) <= 55)
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
      <div
        ref="frameEl"
        class="burnup-frame relative w-full h-44 sm:h-52 rounded-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        role="img"
        tabindex="0"
        :aria-label="summary"
        @pointermove="onMove"
        @pointerleave="onLeave"
        @focus="onFocus"
        @blur="onLeave"
        @keydown="onKeydown"
      >
        <!-- Geometry layer. Stretches to fill; strokes stay crisp via
             non-scaling-stroke (see <style>). Decorative: the frame
             carries the role/label and the sr-only table the data. -->
        <svg
          class="absolute inset-0 h-full w-full overflow-visible"
          :viewBox="`0 0 ${VB_W} ${VB_H}`"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
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

          <!-- Start-scope baseline (creep reference) -->
          <line
            v-if="showBaseline"
            :x1="left"
            :y1="yFor(series.start_scope)"
            :x2="VB_W - right"
            :y2="yFor(series.start_scope)"
            class="stroke-subtle"
            stroke-width="1"
            stroke-dasharray="3 3"
          />

          <!-- Outstanding-work band (scope - completed, elapsed days) -->
          <polygon :points="remainingBand" class="fill-secondary burnup-band" />

          <!-- Today marker line -->
          <line
            v-if="hasFuture"
            :x1="todayX"
            :y1="top"
            :x2="todayX"
            :y2="top + plotH"
            class="stroke-strong"
            stroke-width="1"
            stroke-dasharray="2 3"
            stroke-opacity="0.6"
          />

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
          <polyline :points="scopeLine" fill="none" class="stroke-secondary" stroke-width="1.5" />
          <polyline :points="completedLine" fill="none" class="stroke-accent" stroke-width="2" />

          <!-- Hover crosshair line -->
          <line
            v-if="hoverPoint"
            :x1="hoverX"
            :y1="top"
            :x2="hoverX"
            :y2="top + plotH"
            class="stroke-strong"
            stroke-width="1"
          />
        </svg>

        <!-- Text + marker overlay. Fixed-size type, dots that stay round,
             all positioned by percentage so they track the geometry at
             any width. Non-interactive so pointer events reach the frame. -->
        <div class="pointer-events-none absolute inset-0 text-[10px] leading-none">
          <!-- Y ticks -->
          <span
            class="absolute -translate-y-1/2 text-tertiary tabular-nums"
            :style="{ left: '0', top: `${yPct(yFor(0))}%` }"
            >0</span
          >
          <span
            class="absolute -translate-y-1/2 text-tertiary tabular-nums"
            :style="{ left: '0', top: `${yPct(yFor(series.final_scope))}%` }"
            >{{ series.final_scope }}</span
          >
          <span
            v-if="showBaseline"
            class="absolute -translate-y-1/2 text-tertiary tabular-nums"
            :style="{ left: '0', top: `${yPct(yFor(series.start_scope))}%` }"
            >{{ series.start_scope }}</span
          >

          <!-- Today label -->
          <span
            v-if="hasFuture"
            class="absolute -translate-x-1/2 text-[9px] uppercase tracking-wide text-tertiary"
            :style="{ left: `${xPct(todayX)}%`, top: '0' }"
            >{{ t('cycle-burnup-today') }}</span
          >

          <!-- Direct end-of-line labels (one per series), anchored to the
               right edge so they never overflow. -->
          <span
            class="absolute right-0 -translate-y-1/2 font-medium text-secondary"
            :style="{ top: `${yPct(labelYs.grey)}%` }"
            >{{ hasFuture ? t('cycle-burnup-label-pace') : t('cycle-burnup-legend-scope') }}</span
          >
          <span
            class="absolute right-0 -translate-y-1/2 font-semibold text-accent"
            :style="{ top: `${yPct(labelYs.accent)}%` }"
            >{{ hasFuture ? t('cycle-burnup-label-forecast') : t('cycle-burnup-legend-completed') }}</span
          >

          <!-- Latest-completed marker -->
          <span
            class="absolute h-1.5 w-1.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-accent"
            :style="{ left: `${xPct(todayX)}%`, top: `${yPct(completedLabelY)}%` }"
          />

          <!-- X date labels -->
          <span class="absolute bottom-0 text-tertiary" :style="{ left: `${xPct(left)}%` }">{{
            firstDay
          }}</span>
          <span class="absolute bottom-0 right-0 text-tertiary">{{ endDay }}</span>

          <!-- Hover markers + readout -->
          <template v-if="hoverPoint">
            <span
              class="absolute h-1.5 w-1.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-secondary"
              :style="{ left: `${xPct(hoverX)}%`, top: `${yPct(yFor(hoverPoint.scope))}%` }"
            />
            <span
              class="absolute h-2 w-2 -translate-x-1/2 -translate-y-1/2 rounded-full bg-accent"
              :style="{ left: `${xPct(hoverX)}%`, top: `${yPct(yFor(hoverPoint.completed))}%` }"
            />
            <div
              class="absolute top-1.5 z-10 whitespace-nowrap rounded-md border border-default bg-surface px-2 py-1.5 shadow-sm"
              :style="
                readoutLeft
                  ? { left: `${xPct(hoverX)}%`, marginLeft: '10px' }
                  : { right: `${100 - xPct(hoverX)}%`, marginRight: '10px' }
              "
            >
              <div class="font-medium text-secondary">{{ hoverDay }}</div>
              <div class="text-accent">
                {{ t('cycle-burnup-legend-completed') }}: {{ hoverPoint.completed }}
              </div>
              <div class="text-secondary">
                {{ t('cycle-burnup-legend-scope') }}: {{ hoverPoint.scope }}
              </div>
            </div>
          </template>
        </div>
      </div>

      <!-- Scope-creep callout, promoted from fine print -->
      <p v-if="scopeAdded > 0" class="text-[11px] text-tertiary">
        {{ t('tickets-cycle-scope-added', { count: scopeAdded }) }}
      </p>

      <!-- Visually-hidden data table: full screen-reader + keyboard access. -->
      <table class="sr-only">
        <caption>
          {{
            t('cycle-burnup-table-caption')
          }}
        </caption>
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
/* Crisp lines at any width: the browser keeps stroke widths (and dash
   patterns) in screen pixels rather than scaling them with the SVG. */
.burnup-frame svg :where(line, polyline) {
  vector-effect: non-scaling-stroke;
}

.burnup-band {
  opacity: 0.07;
}

/* Gentle settle-in; degrades to instant under reduced motion. */
.burnup-frame {
  animation: burnup-fade 0.35s ease-out;
}

@keyframes burnup-fade {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .burnup-frame {
    animation: none;
  }
}
</style>
