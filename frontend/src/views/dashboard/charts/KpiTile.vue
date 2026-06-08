<!--
KpiTile — a headline number, a delta vs prior period, and an
optional sparkline.

The shell (DashboardWidgetShell, supplied either by SavedViewWidget
for saved-view-backed kpi tiles or by WidgetFrame's frame-wraps
path for system kpi widgets) owns the title. This component is the
BODY: number, delta, spark. The earlier double-label problem
("Created" in the shell header + "TICKETS CREATED" inside the body)
came from this component carrying its own headline label; the
design language reserves the label for the shell.

Internal state machine (loading / error / data) stays here rather
than the shell because the chart's data fetch is independent of any
shell-level loading (the shell's loading prop is unused on frame-
wraps and SavedViewWidget paths). For chart-config consumers,
viz_config carries the metric + sparkline flag; the time window
comes from useTimeRange so the tile re-fetches on time-range change.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import { useTimeRange } from '@/composables/useTimeRange'
import { useDateStore } from '@/stores/dateStore'
import {
  analyticsService,
  type KpiMetric,
  type KpiResult,
} from '@/services/analyticsService'
import SparklineChart from './SparklineChart.vue'
import {
  buildDashboardMetricDrillDown,
  buildDashboardTicketDrillDown,
} from '@/utils/dashboardTicketDrillDown'

const props = defineProps<{
  metric: KpiMetric
  /** When `true`, the sparkline strip below the number is drawn.
   *  Defaults to `true`; pass `false` for compact tile layouts. */
  showSparkline?: boolean
  /** Optional override for the screen-reader-only label that names
   *  the metric. Shell title supplies the visible label; this is
   *  the accessibility fallback when the tile renders outside a
   *  shell. Defaults to a metric-derived localised string. */
  label?: string
  /** Saved-view uuid to drill into on click. When set, the tile
   *  becomes a `router-link` to `/tickets?view=<uuid>`; otherwise
   *  the tile is non-interactive unless `listViewId` is set. */
  viewUuid?: string
  /** Built-in ticket-list view id for system KPI drill-down
   *  (e.g. `dashboard-created`). Includes the dashboard time
   *  window for created / resolved metrics. */
  listViewId?: string
}>()

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const { window: timeWindow, priorWindow, compare } = useTimeRange()
const dateStore = useDateStore()

const params = computed(() => {
  const w = timeWindow.value
  // Only request prior-period numbers when the user has compare
  // toggled on; skipping the optional params keeps the cache key
  // narrow for the common "no compare" case.
  const includePrior = compare.value
  const p = includePrior ? priorWindow.value : undefined
  return {
    metric: props.metric,
    from: w.from,
    to: w.to,
    prior_from: p?.from,
    prior_to: p?.to,
    sparkline: props.showSparkline !== false,
    // The backend aligns the sparkline's daily buckets to this zone so
    // each dot covers the user's local day, not a UTC day.
    tz: dateStore.effectiveTimezone,
  }
})

const query = useQuery({
  key: () => [
    'dashboard',
    'kpi',
    params.value.metric,
    params.value.from,
    params.value.to,
    params.value.prior_from ?? 'no-prior',
    params.value.prior_to ?? 'no-prior',
    params.value.sparkline ? 'spark' : 'no-spark',
    params.value.tz,
  ],
  query: () => analyticsService.kpi(params.value),
})

// Live refresh is owned at the dashboard root by
// `useDashboardLiveRefresh`: it invalidates the ['dashboard', 'kpi']
// query namespace on ticket mutations, so this tile's query refetches
// without subscribing to SSE itself.

const result = computed<KpiResult | undefined>(() => query.data.value)
const loading = computed(() => query.status.value === 'pending' && !result.value)
const hasError = computed(() => query.status.value === 'error')

const headlineLabel = computed(() => props.label ?? t(`dashboard-kpi-metric-${props.metric}`))

const deltaSign = computed<'up' | 'down' | 'flat' | null>(() => {
  const d = result.value?.delta_value
  if (d == null) return null
  if (d > 0) return 'up'
  if (d < 0) return 'down'
  return 'flat'
})

const deltaPctDisplay = computed<string | null>(() => {
  const pct = result.value?.delta_pct
  if (pct == null) return null
  const formatted = Math.abs(pct).toFixed(1)
  return `${formatted}%`
})

const sparklineValues = computed(() => result.value?.sparkline ?? null)
const showSparklineStrip = computed(
  () => props.showSparkline !== false && (sparklineValues.value?.length ?? 0) > 0,
)

const drillTo = computed(() => {
  if (props.viewUuid) {
    return { path: '/tickets', query: { view: props.viewUuid } }
  }
  if (props.listViewId) {
    return buildDashboardTicketDrillDown(props.listViewId, timeWindow.value)
  }
  if (props.metric) {
    return buildDashboardMetricDrillDown(props.metric, timeWindow.value)
  }
  return undefined
})
</script>

<template>
  <component
    :is="drillTo ? 'router-link' : 'div'"
    :to="drillTo"
    :aria-label="headlineLabel"
    :class="[
      'flex flex-col gap-2 px-4 py-3',
      drillTo
        ? 'transition-colors hover:bg-surface-hover focus-visible:bg-surface-hover focus-visible:outline-none'
        : '',
    ]"
  >
    <p v-if="loading" class="text-metric-md text-tertiary tabular-nums">
      &mdash;
    </p>
    <p v-else-if="hasError" class="text-xs text-status-error">
      {{ t('dashboard-kpi-error') }}
    </p>
    <p v-else class="text-metric-md text-primary tabular-nums">
      {{ result?.value ?? 0 }}
    </p>

    <div
      v-if="deltaSign && deltaPctDisplay"
      :class="[
        'flex items-center gap-1 text-[11px] font-medium tabular-nums',
        deltaSign === 'up' ? 'text-status-success' : '',
        deltaSign === 'down' ? 'text-status-error' : '',
        deltaSign === 'flat' ? 'text-tertiary' : '',
      ]"
    >
      <span v-if="deltaSign === 'up'" aria-hidden="true">▲</span>
      <span v-else-if="deltaSign === 'down'" aria-hidden="true">▼</span>
      <span v-else aria-hidden="true">▬</span>
      <span>{{ deltaPctDisplay }}</span>
    </div>

    <SparklineChart v-if="showSparklineStrip" :values="sparklineValues" :height="28" />
  </component>
</template>
