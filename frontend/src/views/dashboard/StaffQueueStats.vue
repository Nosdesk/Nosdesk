<!--
Queue stats rail. Unlike the other stat widgets this one is user-
configurable: the header carries a gear that opens a picker, letting
users pick which KPIs appear (up to MAX_METRICS). The chosen set is
persisted in `dashboard_layout.widgets[...].config.metrics` and falls
back to a sensible default for unconfigured users.

The metric catalog lives in this file because it is Queue-specific
(labels, tones, which slice of `QueueStats` to read). Adding a new
metric is a one-entry append, the picker reflects it automatically.

Counts come from the dashboard coordinator (one shared
`/api/dashboard/stats?include=queue` request across all stat widgets),
injected via `useInjectedDashboardStats()`.
-->
<script setup lang="ts">
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useInjectedDashboardStats } from '@/composables/useDashboardStats'
import type { QueueStats } from '@/services/dashboardService'
import { useWidgetConfigState } from '@/composables/useWidgetConfigState'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import KpiRail, { type Kpi } from './KpiRail.vue'
import KpiRailSkeleton from './KpiRailSkeleton.vue'
import QueueMetricsPicker from './QueueMetricsPicker.vue'
import Icon from '@/components/common/Icon.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const WIDGET_ID = 'stats-queue'
const MAX_METRICS = 4

interface QueueMetric {
  id: string
  label: string
  description: string
  tone: string
  to: string
  pick: (q: QueueStats) => number
}

// Catalog, order here is the canonical display order the picker uses
// when rendering options. A user's selection is stored as an array of
// ids and always rendered in catalog order (see `kpis` below), so
// adding a new metric is a backward-compatible append.
const CATALOG = computed<QueueMetric[]>(() => [
  {
    id: 'unassigned',
    label: t('dashboard-staff-queue-metric-unassigned-label'),
    description: t('dashboard-staff-queue-metric-unassigned-desc'),
    tone: 'text-status-error',
    to: '/tickets?assignee=unassigned&status=open',
    pick: (q) => q.unassigned,
  },
  {
    id: 'all',
    label: t('dashboard-staff-queue-metric-all-label'),
    description: t('dashboard-staff-queue-metric-all-desc'),
    tone: 'text-primary',
    to: '/tickets',
    pick: (q) => q.total,
  },
  {
    id: 'open',
    label: t('dashboard-staff-queue-metric-open-label'),
    description: t('dashboard-staff-queue-metric-open-desc'),
    tone: 'text-status-open',
    to: '/tickets?status=open',
    pick: (q) => q.open,
  },
  {
    id: 'in-progress',
    label: t('dashboard-staff-queue-metric-in-progress-label'),
    description: t('dashboard-staff-queue-metric-in-progress-desc'),
    tone: 'text-status-in-progress',
    to: '/tickets?status=in-progress',
    pick: (q) => q.inProgress,
  },
  {
    id: 'high-priority',
    label: t('dashboard-staff-queue-metric-high-priority-label'),
    description: t('dashboard-staff-queue-metric-high-priority-desc'),
    tone: 'text-priority-high',
    to: '/tickets?priority=high&status=open',
    pick: (q) => q.highPriority,
  },
  {
    id: 'closed-today',
    label: t('dashboard-staff-queue-metric-closed-today-label'),
    description: t('dashboard-staff-queue-metric-closed-today-desc'),
    tone: 'text-status-closed',
    to: '/tickets?status=closed',
    pick: (q) => q.closedToday,
  },
])

const DEFAULT_METRIC_IDS: string[] = ['unassigned', 'all']

const EMPTY_QUEUE: QueueStats = {
  total: 0,
  unassigned: 0,
  open: 0,
  inProgress: 0,
  highPriority: 0,
  closedToday: 0,
}

const stats = useInjectedDashboardStats()
const queue = computed<QueueStats>(() => stats.bundle.value?.queue ?? EMPTY_QUEUE)
const errorMessage = computed(() => (stats.isError.value ? t('dashboard-staff-queue-error') : null))

const config = useWidgetConfigState(WIDGET_ID, {
  metrics: DEFAULT_METRIC_IDS,
})

// Validate the persisted array on read, `useWidgetConfigState` keeps
// hydrated values verbatim, so defensive type narrowing happens here.
const selectedMetricIds = computed<string[]>(() => {
  const raw = Array.isArray(config.metrics) ? config.metrics : []
  const ids = raw.filter((v): v is string => typeof v === 'string')
  return ids.length > 0 ? ids.slice(0, MAX_METRICS) : DEFAULT_METRIC_IDS
})

const kpis = computed<Kpi[]>(() =>
  selectedMetricIds.value
    .map((id) => CATALOG.value.find((m) => m.id === id))
    .filter((m): m is QueueMetric => !!m)
    .map((m) => ({
      id: m.id,
      label: m.label,
      description: m.description,
      tone: m.tone,
      to: m.to,
      value: m.pick(queue.value),
    })),
)

const pickerOpen = ref(false)

function savePicker(newIds: string[]) {
  config.metrics = newIds.slice(0, MAX_METRICS)
  pickerOpen.value = false
}
</script>

<template>
  <DashboardWidgetShell
    :title="t('dashboard-staff-queue-title')"
    :loading="stats.isLoading.value"
    :refreshing="stats.isRefreshing.value"
    :error="errorMessage"
  >
    <!-- Gear in the header opens the metric picker. Lives here (not
         tied to dashboard edit mode) because picking metrics is a
         quick, in-context action, not a layout change. -->
    <template #headerActions>
      <button
        type="button"
        class="flex items-center justify-center w-5 h-5 rounded text-tertiary hover:text-primary hover:bg-surface transition-colors"
        :aria-label="t('dashboard-staff-queue-configure-aria')"
        :title="t('dashboard-staff-queue-configure-title')"
        @click="pickerOpen = true"
      >
        <Icon name="settings" />
      </button>
    </template>

    <template #skeleton>
      <KpiRailSkeleton :count="selectedMetricIds.length" />
    </template>

    <KpiRail :kpis="kpis" />

    <QueueMetricsPicker
      :show="pickerOpen"
      :catalog="CATALOG"
      :selected-ids="selectedMetricIds"
      :max="MAX_METRICS"
      @save="savePicker"
      @close="pickerOpen = false"
    />
  </DashboardWidgetShell>
</template>
