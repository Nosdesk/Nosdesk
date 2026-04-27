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
import { useInjectedDashboardStats } from '@/composables/useDashboardStats'
import type { QueueStats } from '@/services/dashboardService'
import { useWidgetConfigState } from '@/composables/useWidgetConfigState'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import KpiRail, { type Kpi } from './KpiRail.vue'
import KpiRailSkeleton from './KpiRailSkeleton.vue'
import QueueMetricsPicker from './QueueMetricsPicker.vue'

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
const CATALOG: QueueMetric[] = [
  {
    id: 'unassigned',
    label: 'Unassigned',
    description: 'Open, no assignee',
    tone: 'text-status-error',
    to: '/tickets?assignee=unassigned&status=open',
    pick: (q) => q.unassigned,
  },
  {
    id: 'all',
    label: 'All Tickets',
    description: 'Every status',
    tone: 'text-primary',
    to: '/tickets',
    pick: (q) => q.total,
  },
  {
    id: 'open',
    label: 'Open',
    description: 'Status: open',
    tone: 'text-status-open',
    to: '/tickets?status=open',
    pick: (q) => q.open,
  },
  {
    id: 'in-progress',
    label: 'In Progress',
    description: 'Currently being worked',
    tone: 'text-status-in-progress',
    to: '/tickets?status=in-progress',
    pick: (q) => q.inProgress,
  },
  {
    id: 'high-priority',
    label: 'High Priority',
    description: 'High priority, still open',
    tone: 'text-priority-high',
    to: '/tickets?priority=high&status=open',
    pick: (q) => q.highPriority,
  },
  {
    id: 'closed-today',
    label: 'Closed Today',
    description: 'Closed in the last 24h',
    tone: 'text-status-closed',
    to: '/tickets?status=closed',
    pick: (q) => q.closedToday,
  },
]

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
const errorMessage = computed(() => (stats.isError.value ? 'Failed to load queue metrics' : null))

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
    .map((id) => CATALOG.find((m) => m.id === id))
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
    title="Queue"
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
        aria-label="Configure queue metrics"
        title="Configure metrics"
        @click="pickerOpen = true"
      >
        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
          <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
        </svg>
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
