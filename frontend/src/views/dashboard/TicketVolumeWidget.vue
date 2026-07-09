<!--
Grouped ticket-volume widget: created, resolved, and open in one
card with compare deltas, sparklines, and drill-down links. Replaces
the three separate KpiTile widgets in the default staff layout while
keeping those tiles available in the widget picker for users who
prefer them split.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useTimeRange } from '@/composables/useTimeRange'
import { useTicketVolumeKpis } from '@/composables/useTicketVolumeKpis'
import { buildDashboardMetricDrillDown } from '@/utils/dashboardTicketDrillDown'
import type { KpiResult } from '@/services/analyticsService'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import VolumeKpiRail, { type VolumeKpi } from './charts/VolumeKpiRail.vue'
import VolumeKpiRailSkeleton from './charts/VolumeKpiRailSkeleton.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const { compare, window: timeWindow } = useTimeRange()
const { data, status, error } = useTicketVolumeKpis()

const loading = computed(() => status.value === 'pending' && !data.value)
const isRefreshing = computed(() => status.value === 'pending' && !!data.value)
const errorMessage = computed(() => (error.value ? t('dashboard-kpi-error') : null))

function deltaSign(result: KpiResult | undefined): 'up' | 'down' | 'flat' | null {
  const d = result?.delta_value
  if (d == null) return null
  if (d > 0) return 'up'
  if (d < 0) return 'down'
  return 'flat'
}

function deltaPctDisplay(result: KpiResult | undefined): string | null {
  const pct = result?.delta_pct
  if (pct == null) return null
  return `${Math.abs(pct).toFixed(1)}%`
}

const kpis = computed<VolumeKpi[]>(() => {
  const bundle = data.value
  const window = timeWindow.value
  return [
    {
      id: 'created',
      label: t('dashboard-ticket-volume-created'),
      value: bundle?.created.value ?? 0,
      to: buildDashboardMetricDrillDown('tickets_created', window),
      description: t('dashboard-ticket-volume-created-hint'),
      deltaSign: compare.value ? deltaSign(bundle?.created) : null,
      deltaPctDisplay: compare.value ? deltaPctDisplay(bundle?.created) : null,
      sparkline: bundle?.created.sparkline,
    },
    {
      id: 'resolved',
      label: t('dashboard-ticket-volume-resolved'),
      value: bundle?.resolved.value ?? 0,
      to: buildDashboardMetricDrillDown('tickets_resolved', window),
      description: t('dashboard-ticket-volume-resolved-hint'),
      deltaSign: compare.value ? deltaSign(bundle?.resolved) : null,
      deltaPctDisplay: compare.value ? deltaPctDisplay(bundle?.resolved) : null,
      sparkline: bundle?.resolved.sparkline,
    },
    {
      id: 'open',
      label: t('dashboard-ticket-volume-open'),
      value: bundle?.open.value ?? 0,
      to: buildDashboardMetricDrillDown('tickets_open'),
      description: t('dashboard-ticket-volume-open-hint'),
      // Backlog trend (open at each day's end), reconstructed server-side
      // in kpi_summary alongside created/resolved.
      sparkline: bundle?.open.sparkline ?? null,
    },
  ]
})
</script>

<template>
  <DashboardWidgetShell
    :title="t('dashboard-widget-ticket-volume-title')"
    :action-label="t('dashboard-ticket-volume-view-all')"
    action-to="/tickets"
    :loading="loading"
    :refreshing="isRefreshing"
    :error="errorMessage"
    flush-body
  >
    <template #skeleton><VolumeKpiRailSkeleton /></template>
    <VolumeKpiRail :kpis="kpis" />
  </DashboardWidgetShell>
</template>
