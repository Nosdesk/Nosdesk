<!--
TicketFlowChart — net ticket flow (created − resolved) per period as a
diverging area around a central zero axis.

Net flow is the rate the backlog changes: above the axis more tickets
were opened than closed (backlog grew), below it more were closed than
opened (backlog shrank). The cumulative of this is the Open KPI's
backlog level. Reading "are we keeping up?" is a glance — which side of
the axis the fill sits on, and how far.

Design notes:
- Symmetric scale (zero centred) so above/below are directly comparable.
- Straight segments, not smoothing: net is a discrete per-period delta,
  and a monotone curve would overshoot across the axis.
- The area is filled once and clipped to the above/below-axis halves,
  so a segment that crosses zero is split at the exact crossing with no
  hand-computed intersections. Above = accent (created-led), below =
  green (resolved-led), echoing the created/resolved colours.
- Grain widens with the range (day → week → month) so a quarter or a
  year is a readable dozen-ish buckets, not 90–365 spikes.
- The trailing in-progress bucket (today / current hour) is dropped when
  the window ends "now", so a half-finished period doesn't read as a
  real swing to zero.
-->
<script setup lang="ts">
import { computed, ref } from 'vue'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import { useTimeRange } from '@/composables/useTimeRange'
import { useElementSize } from '@/composables/useElementSize'
import { useDateStore } from '@nosdesk/core/stores/dateStore'
import {
  analyticsService,
  type TsTimeField,
  type TimeseriesBucket,
} from '@/services/analyticsService'
import { monotonePath } from './seriesPath'

let uidCounter = 0
function nextUid(): number {
  uidCounter += 1
  return uidCounter
}

const props = withDefaults(
  defineProps<{
    /** Saved-view uuid for drill-through to the filtered ticket list. */
    viewUuid?: string
  }>(),
  { viewUuid: undefined },
)

const fluent = useFluent()
const t = (k: string) => fluent.$t(k)

const uid = nextUid()
const clipAboveId = `flow-above-${uid}`
const clipBelowId = `flow-below-${uid}`

const { window: timeWindow, grain } = useTimeRange()
const dateStore = useDateStore()
const tz = computed(() => dateStore.effectiveTimezone)

const spanDays = computed(() =>
  Math.max(
    1,
    Math.round(
      (new Date(timeWindow.value.to).getTime() - new Date(timeWindow.value.from).getTime()) /
        86_400_000,
    ),
  ),
)

// Widen the bucket with the range so a long window stays legible.
type DisplayGrain = 'hour' | 'day' | 'week' | 'month'
const displayGrain = computed<DisplayGrain>(() => {
  if (grain.value === 'hour') return 'hour'
  if (spanDays.value <= 45) return 'day'
  if (spanDays.value <= 180) return 'week'
  return 'month'
})

const GRAIN_MS: Record<DisplayGrain, number> = {
  hour: 3_600_000,
  day: 86_400_000,
  week: 7 * 86_400_000,
  month: 30 * 86_400_000,
}

function seriesQuery(timeField: TsTimeField) {
  return useQuery({
    key: () => [
      'dashboard',
      'timeseries',
      'count',
      timeField,
      displayGrain.value,
      tz.value,
      timeWindow.value.from,
      timeWindow.value.to,
    ],
    query: () =>
      analyticsService.timeseries({
        measure: 'count',
        time_field: timeField,
        from: timeWindow.value.from,
        to: timeWindow.value.to,
        grain: displayGrain.value,
        tz: tz.value,
      }),
  })
}

const createdQuery = seriesQuery('created_at')
const resolvedQuery = seriesQuery('closed_at')

const created = computed<TimeseriesBucket[]>(() => createdQuery.data.value?.buckets ?? [])
const resolved = computed<TimeseriesBucket[]>(() => resolvedQuery.data.value?.buckets ?? [])

interface NetBucket {
  ts: string
  net: number
}

// Net = created − resolved per aligned bucket, dropping the trailing
// in-progress period when the window ends at "now".
const netBuckets = computed<NetBucket[]>(() => {
  const c = created.value
  const r = resolved.value
  const n = Math.min(c.length, r.length)
  if (n === 0) return []
  const out: NetBucket[] = []
  for (let i = 0; i < n; i += 1) out.push({ ts: c[i].ts, net: c[i].value - r[i].value })
  const endsNow = Date.now() - new Date(timeWindow.value.to).getTime() < GRAIN_MS[displayGrain.value]
  if (endsNow && out.length > 1) out.pop()
  return out
})

const loading = computed(
  () =>
    (createdQuery.status.value === 'pending' && created.value.length === 0) ||
    (resolvedQuery.status.value === 'pending' && resolved.value.length === 0),
)
const hasError = computed(
  () => createdQuery.status.value === 'error' || resolvedQuery.status.value === 'error',
)
const isEmpty = computed(
  () => !loading.value && !hasError.value && netBuckets.value.every((b) => b.net === 0),
)

const containerRef = ref<HTMLElement | null>(null)
const { width, height } = useElementSize(containerRef)

const PAD_LEFT = 32
const PAD_RIGHT = 12
const PAD_TOP = 12
const PAD_BOTTOM = 22
const AXIS_FONT = 11
const innerW = computed(() => Math.max(0, width.value - PAD_LEFT - PAD_RIGHT))
const innerH = computed(() => Math.max(0, height.value - PAD_TOP - PAD_BOTTOM))
interface Pt {
  x: number
  y: number
}

// Data-fit scale: the plot spans the net series' own [min..max], always
// including zero so the axis stays meaningful. Zero floats to its true
// proportional position rather than being pinned to the centre, so a
// mostly one-sided series fills the height instead of hugging a half.
const scale = computed(() => {
  const values = netBuckets.value.map((b) => b.net)
  const top = Math.max(0, ...values)
  const bottom = Math.min(0, ...values)
  const range = top - bottom || 1
  const h = innerH.value
  const y = (v: number) => PAD_TOP + ((top - v) / range) * h
  return { top, bottom, zeroY: y(0), y }
})

const zeroY = computed(() => scale.value.zeroY)

const chart = computed(() => {
  const data = netBuckets.value
  if (data.length === 0 || innerW.value <= 0 || innerH.value <= 0) {
    return { areaPath: '', linePath: '', last: null as Pt | null }
  }
  const w = innerW.value
  const z = scale.value.zeroY
  const yOf = scale.value.y
  const step = data.length > 1 ? w / (data.length - 1) : 0
  const pts: Pt[] = data.map((b, i) => ({
    x: PAD_LEFT + i * step,
    y: yOf(b.net),
  }))
  // Monotone smoothing to match the other charts. It can't overshoot
  // past a data point, so the fill never extends beyond the real peak /
  // trough, and the clip halves colour the curve correctly wherever it
  // crosses the axis.
  const linePath = monotonePath(pts)
  // Close the line down to the zero axis so the enclosed region is the
  // area between the curve and zero; the two clips colour each half.
  const first = pts[0]
  const lastPt = pts[pts.length - 1]
  const areaPath = `${linePath} L${lastPt.x.toFixed(1)},${z.toFixed(1)} L${first.x.toFixed(1)},${z.toFixed(1)} Z`
  return { areaPath, linePath, last: lastPt }
})

interface XTick {
  x: number
  label: string
  anchor: 'start' | 'middle' | 'end'
}

const MIN_LABEL_GAP = 64

const xLabels = computed<XTick[]>(() => {
  const data = netBuckets.value
  if (data.length === 0 || innerW.value <= 0) return []
  const n = data.length
  const step = n > 1 ? innerW.value / (n - 1) : 0
  const xAt = (i: number) => PAD_LEFT + i * step
  const ts = (i: number) => new Date(data[i].ts)

  const g = displayGrain.value
  const fmt = (i: number): string => {
    const d = ts(i)
    if (g === 'hour') return d.toLocaleTimeString(dateStore.locale, { hour: 'numeric', timeZone: tz.value })
    if (g === 'month') return d.toLocaleDateString(dateStore.locale, { month: 'short', year: '2-digit', timeZone: tz.value })
    return d.toLocaleDateString(dateStore.locale, { month: 'short', day: 'numeric', timeZone: tz.value })
  }

  const kept: number[] = []
  for (let i = 0; i < n; i += 1) {
    if (kept.length === 0 || xAt(i) - xAt(kept[kept.length - 1]) >= MIN_LABEL_GAP) kept.push(i)
  }
  if (kept[kept.length - 1] !== n - 1) {
    if (kept.length > 0 && xAt(n - 1) - xAt(kept[kept.length - 1]) < MIN_LABEL_GAP) kept.pop()
    kept.push(n - 1)
  }

  return kept.map((i, k) => ({
    x: xAt(i),
    label: fmt(i),
    anchor: k === 0 ? 'start' : k === kept.length - 1 ? 'end' : 'middle',
  }))
})

// ── Hover tooltip ──────────────────────────────────────────────────
// Nearest-bucket crosshair: the SVG fills its box 1:1 (viewBox === pixel
// size), so cursor-x within the plot maps straight to chart-x. The
// tooltip surfaces the created/resolved magnitudes the net view folds
// away, plus the net with its grew/shrank sense.

const hoverIndex = ref<number | null>(null)
const hoverClient = ref({ x: 0, y: 0 })
const tooltipRef = ref<HTMLElement | null>(null)

function onHoverMove(e: MouseEvent): void {
  const data = netBuckets.value
  if (data.length === 0 || innerW.value <= 0) {
    hoverIndex.value = null
    return
  }
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
  const x = e.clientX - rect.left
  const step = data.length > 1 ? innerW.value / (data.length - 1) : 0
  const i = step > 0 ? Math.round((x - PAD_LEFT) / step) : 0
  hoverIndex.value = Math.max(0, Math.min(data.length - 1, i))
  hoverClient.value = { x: e.clientX, y: e.clientY }
}

function onHoverLeave(): void {
  hoverIndex.value = null
}

const hoverPt = computed<Pt | null>(() => {
  const i = hoverIndex.value
  const data = netBuckets.value
  if (i == null || data.length === 0) return null
  const step = data.length > 1 ? innerW.value / (data.length - 1) : 0
  return { x: PAD_LEFT + i * step, y: scale.value.y(data[i].net) }
})

const hoverInfo = computed(() => {
  const i = hoverIndex.value
  if (i == null) return null
  const nb = netBuckets.value[i]
  if (!nb) return null
  return {
    date: fullDate(nb.ts),
    created: created.value[i]?.value ?? 0,
    resolved: resolved.value[i]?.value ?? 0,
    net: nb.net,
  }
})

/** Longer date label for the tooltip (the axis uses the terse form). */
function fullDate(tsStr: string): string {
  const d = new Date(tsStr)
  const g = displayGrain.value
  if (g === 'hour')
    return d.toLocaleString(dateStore.locale, {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      timeZone: tz.value,
    })
  if (g === 'month')
    return d.toLocaleDateString(dateStore.locale, { month: 'long', year: 'numeric', timeZone: tz.value })
  return d.toLocaleDateString(dateStore.locale, {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    timeZone: tz.value,
  })
}

// Cursor-following card, flipped away from the viewport edges (mirrors
// HeatmapTooltip). Recomputed each move; the ref may be null on the
// first frame, in which case no flip is applied (harmless).
const tooltipStyle = computed(() => {
  let x = hoverClient.value.x + 12
  let y = hoverClient.value.y + 12
  const el = tooltipRef.value
  if (el) {
    if (x + el.offsetWidth > window.innerWidth) x = hoverClient.value.x - el.offsetWidth - 12
    if (y + el.offsetHeight > window.innerHeight) y = hoverClient.value.y - el.offsetHeight - 12
  }
  return { left: `${x}px`, top: `${y}px` }
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
    <component
      v-else
      :is="props.viewUuid ? 'router-link' : 'div'"
      :to="props.viewUuid ? { path: '/tickets', query: { view: props.viewUuid } } : undefined"
      :class="[
        'relative block w-full h-full overflow-hidden',
        props.viewUuid ? 'transition-colors hover:bg-surface-hover rounded' : '',
      ]"
    >
      <div ref="containerRef" class="absolute inset-0" @mousemove="onHoverMove" @mouseleave="onHoverLeave">
        <svg
          v-if="width > 0 && height > 0"
          :viewBox="`0 0 ${width} ${height}`"
          class="block w-full h-full"
          role="img"
          :aria-label="t('dashboard-flow-chart-aria-label')"
        >
          <defs>
            <!-- Halves split at the zero axis; the single area path is
                 drawn twice, each clipped to one half, for two-tone fill. -->
            <clipPath :id="clipAboveId">
              <rect :x="PAD_LEFT" :y="PAD_TOP" :width="innerW" :height="Math.max(0, zeroY - PAD_TOP)" />
            </clipPath>
            <clipPath :id="clipBelowId">
              <rect :x="PAD_LEFT" :y="zeroY" :width="innerW" :height="Math.max(0, PAD_TOP + innerH - zeroY)" />
            </clipPath>
          </defs>

          <!-- Above-axis fill (created-led → backlog grew). -->
          <path
            v-if="chart.areaPath"
            :d="chart.areaPath"
            class="text-accent"
            fill="currentColor"
            fill-opacity="0.22"
            stroke="none"
            :clip-path="`url(#${clipAboveId})`"
          />
          <!-- Below-axis fill (resolved-led → backlog shrank). -->
          <path
            v-if="chart.areaPath"
            :d="chart.areaPath"
            class="text-status-success"
            fill="currentColor"
            fill-opacity="0.22"
            stroke="none"
            :clip-path="`url(#${clipBelowId})`"
          />

          <!-- Y labels: peak surplus / 0 / peak deficit, at their true
               positions on the data-fit scale. -->
          <g class="text-tertiary" :font-size="AXIS_FONT">
            <text v-if="scale.top > 0" :x="PAD_LEFT - 6" :y="PAD_TOP + AXIS_FONT" text-anchor="end" fill="currentColor">
              +{{ scale.top }}
            </text>
            <text :x="PAD_LEFT - 6" :y="zeroY + AXIS_FONT / 3" text-anchor="end" fill="currentColor">0</text>
            <text v-if="scale.bottom < 0" :x="PAD_LEFT - 6" :y="PAD_TOP + innerH - 2" text-anchor="end" fill="currentColor">
              −{{ Math.abs(scale.bottom) }}
            </text>
          </g>

          <!-- The central zero axis, drawn over the fills. -->
          <line
            :x1="PAD_LEFT"
            :x2="width - PAD_RIGHT"
            :y1="zeroY"
            :y2="zeroY"
            class="text-default"
            stroke="currentColor"
            stroke-width="1.25"
            opacity="0.7"
          />

          <!-- The net line itself, for shape definition. -->
          <path
            v-if="chart.linePath"
            :d="chart.linePath"
            fill="none"
            stroke="currentColor"
            stroke-width="1.75"
            stroke-linejoin="round"
            stroke-linecap="round"
            class="text-secondary"
            opacity="0.7"
          />

          <!-- X labels along the bottom. -->
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

          <!-- Endpoint marker on the latest complete period. -->
          <circle v-if="chart.last" :cx="chart.last.x" :cy="chart.last.y" r="3.5" class="text-surface" fill="currentColor" />
          <circle v-if="chart.last" :cx="chart.last.x" :cy="chart.last.y" r="2" class="text-secondary" fill="currentColor" />

          <!-- Hover crosshair + marker at the nearest bucket. -->
          <g v-if="hoverPt" class="pointer-events-none">
            <line
              :x1="hoverPt.x"
              :x2="hoverPt.x"
              :y1="PAD_TOP"
              :y2="PAD_TOP + innerH"
              class="text-tertiary"
              stroke="currentColor"
              stroke-width="1"
              stroke-dasharray="3,2"
              opacity="0.7"
            />
            <circle
              :cx="hoverPt.x"
              :cy="hoverPt.y"
              r="4"
              :class="(hoverInfo?.net ?? 0) >= 0 ? 'text-accent' : 'text-status-success'"
              fill="currentColor"
            />
          </g>
        </svg>
      </div>
    </component>

    <!-- Floating tooltip: teleported out so a transformed/overflow-hidden
         widget ancestor can't clip or reposition it. -->
    <Teleport to="body">
      <div
        v-if="hoverInfo"
        ref="tooltipRef"
        class="fixed z-overlay pointer-events-none rounded-lg border border-default bg-surface p-2.5 shadow-lg min-w-[160px]"
        :style="tooltipStyle"
      >
        <div class="mb-1.5 text-xs font-medium text-secondary">{{ hoverInfo.date }}</div>
        <div class="flex items-center justify-between gap-4 text-sm">
          <span class="flex items-center gap-1.5 text-secondary">
            <span class="inline-block h-2 w-2 rounded-full bg-accent" aria-hidden="true"></span>
            {{ t('dashboard-ticket-volume-created') }}
          </span>
          <span class="font-medium tabular-nums text-primary">{{ hoverInfo.created }}</span>
        </div>
        <div class="flex items-center justify-between gap-4 text-sm">
          <span class="flex items-center gap-1.5 text-secondary">
            <span class="inline-block h-2 w-2 rounded-full bg-status-success" aria-hidden="true"></span>
            {{ t('dashboard-ticket-volume-resolved') }}
          </span>
          <span class="font-medium tabular-nums text-primary">{{ hoverInfo.resolved }}</span>
        </div>
        <div class="mt-1.5 flex items-center justify-between gap-4 border-t border-subtle pt-1.5 text-sm">
          <span class="text-tertiary">
            {{ hoverInfo.net > 0 ? t('dashboard-flow-net-grew') : hoverInfo.net < 0 ? t('dashboard-flow-net-shrank') : t('dashboard-flow-net-balanced') }}
          </span>
          <span
            class="font-semibold tabular-nums"
            :class="hoverInfo.net > 0 ? 'text-accent' : hoverInfo.net < 0 ? 'text-status-success' : 'text-tertiary'"
          >
            {{ hoverInfo.net > 0 ? `+${hoverInfo.net}` : hoverInfo.net }}
          </span>
        </div>
      </div>
    </Teleport>
  </div>
</template>
