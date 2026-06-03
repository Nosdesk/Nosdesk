<!--
KpiTile — a headline number, a delta vs prior period, and an
optional sparkline. Renders inside the SavedViewWidget shell when
a saved view's viz_type is `kpi_tile`.

The component intentionally does its own state machine (loading /
error / data) at this layer rather than relying on the parent
shell, because KpiTile is also used directly in places that aren't
saved-view-backed (the KpiRail on the dashboard's "Today" section
in Wave 8). For chart-config consumers, viz_config carries the
metric + sparkline flag; the time window comes from useTimeRange so
the tile re-fetches when the dashboard's time range changes.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import { useTimeRange } from '@/composables/useTimeRange'
import { useLiveKpi } from '@/composables/useLiveKpi'
import {
  analyticsService,
  type KpiMetric,
  type KpiResult,
} from '@/services/analyticsService'

const props = defineProps<{
  metric: KpiMetric
  /** When `true`, the sparkline strip below the number is drawn.
   *  Defaults to `true`; pass `false` for compact tile layouts. */
  showSparkline?: boolean
  /** Optional override for the headline label. Defaults to a
   *  metric-derived localised string. */
  label?: string
  /** Saved-view uuid to drill into on click. When set, the tile
   *  becomes a `router-link` to `/tickets?view=<uuid>`; otherwise
   *  the tile is non-interactive. */
  viewUuid?: string
}>()

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const { window: timeWindow, priorWindow, compare } = useTimeRange()

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
  ],
  query: () => analyticsService.kpi(params.value),
})

// Live refresh: ticket mutations (created / updated / deleted) on
// any tab connected to this workspace nudge the KPI to refetch.
// Debounced inside the composable so a burst of state changes
// triggers one trailing fetch.
useLiveKpi({ onRefresh: () => query.refetch() })

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

/**
 * Spark path: a simple polyline through normalised (x, y) pairs.
 * Tiny scale (24px height), no axes, no labels — the headline
 * number carries the story and the spark only shows shape.
 */
const sparkPath = computed<string | null>(() => {
  const values = result.value?.sparkline
  if (!values || values.length === 0) return null
  const w = 100
  const h = 24
  const max = Math.max(...values, 1)
  const step = values.length > 1 ? w / (values.length - 1) : 0
  return values
    .map((v, i) => {
      const x = i * step
      const y = h - (v / max) * h
      return `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`
    })
    .join(' ')
})
</script>

<template>
  <component
    :is="viewUuid ? 'router-link' : 'div'"
    :to="viewUuid ? { path: '/tickets', query: { view: viewUuid } } : undefined"
    :class="[
      'flex flex-col gap-1 p-4',
      viewUuid
        ? 'transition-colors hover:bg-surface-hover focus-visible:bg-surface-hover focus-visible:outline-none'
        : '',
    ]"
  >
    <div class="flex items-baseline justify-between gap-2">
      <span class="text-xs uppercase tracking-wide text-tertiary truncate">
        {{ headlineLabel }}
      </span>
      <span
        v-if="deltaSign && deltaPctDisplay"
        :class="[
          'text-[11px] font-medium tabular-nums',
          deltaSign === 'up' ? 'text-status-success' : '',
          deltaSign === 'down' ? 'text-status-error' : '',
          deltaSign === 'flat' ? 'text-tertiary' : '',
        ]"
      >
        <span v-if="deltaSign === 'up'" aria-hidden="true">▲</span>
        <span v-else-if="deltaSign === 'down'" aria-hidden="true">▼</span>
        <span v-else aria-hidden="true">▬</span>
        {{ deltaPctDisplay }}
      </span>
    </div>

    <p v-if="loading" class="text-2xl font-semibold text-tertiary tabular-nums">
      &mdash;
    </p>
    <p v-else-if="hasError" class="text-xs text-status-error">
      {{ t('dashboard-kpi-error') }}
    </p>
    <p v-else class="text-2xl font-semibold text-primary tabular-nums">
      {{ result?.value ?? 0 }}
    </p>

    <svg
      v-if="sparkPath"
      class="w-full h-6 mt-1"
      viewBox="0 0 100 24"
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <path :d="sparkPath" fill="none" stroke="currentColor" stroke-width="1.2" class="text-accent" />
    </svg>
  </component>
</template>
