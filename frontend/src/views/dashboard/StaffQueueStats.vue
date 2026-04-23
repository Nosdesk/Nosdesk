<!--
Queue stats rail. Unlike the other stat widgets this one is user-
configurable: the header carries a gear that opens a picker, letting
users pick which KPIs appear (up to MAX_METRICS). The chosen set is
persisted in `dashboard_layout.widgets[...].config.metrics` and falls
back to a sensible default for unconfigured users.

The metric catalog lives in this file because it is Queue-specific
(shape of `Ticket` it operates over, labels, tones). Adding a new
metric is a one-entry append — the picker reflects it automatically.

Fetch + SSE plumbing is shared via `useTicketStats`; layout and
skeleton are shared via `KpiRail` / `KpiRailSkeleton`.
-->
<script setup lang="ts">
import { computed, ref } from 'vue'
import type { Ticket } from '@/services/ticketService'
import { useTicketStats } from '@/composables/useTicketStats'
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
  compute: (tickets: Ticket[]) => number
}

// Catalog — order here is the canonical display order the picker uses
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
    compute: (t) => t.filter((x) => !x.assignee && x.status === 'open').length,
  },
  {
    id: 'all',
    label: 'All Tickets',
    description: 'Every status',
    tone: 'text-primary',
    to: '/tickets',
    compute: (t) => t.length,
  },
  {
    id: 'open',
    label: 'Open',
    description: 'Status: open',
    tone: 'text-status-open',
    to: '/tickets?status=open',
    compute: (t) => t.filter((x) => x.status === 'open').length,
  },
  {
    id: 'in-progress',
    label: 'In Progress',
    description: 'Currently being worked',
    tone: 'text-status-in-progress',
    to: '/tickets?status=in-progress',
    compute: (t) => t.filter((x) => x.status === 'in-progress').length,
  },
  {
    id: 'high-priority',
    label: 'High Priority',
    description: 'High or critical, still open',
    tone: 'text-priority-high',
    to: '/tickets?priority=high&status=open',
    compute: (t) =>
      t.filter((x) => {
        const p = x.priority as string
        return (p === 'high' || p === 'critical') && x.status !== 'closed'
      }).length,
  },
  {
    id: 'closed-today',
    label: 'Closed Today',
    description: 'Closed in the last 24h',
    tone: 'text-status-closed',
    to: '/tickets?status=closed',
    compute: (t) => {
      const dayAgo = Date.now() - 24 * 60 * 60 * 1000
      return t.filter(
        (x) => x.status === 'closed' && new Date(x.modified).getTime() > dayAgo,
      ).length
    },
  },
]

const DEFAULT_METRIC_IDS: string[] = ['unassigned', 'all']

// Raw ticket set; metrics are derived from it so picker toggles
// update the rail instantly without refetching.
const { data: tickets, loading, error } = useTicketStats<Ticket[]>(
  (all) => all,
  [],
  'Failed to load queue metrics',
)

const config = useWidgetConfigState(WIDGET_ID, {
  metrics: DEFAULT_METRIC_IDS,
})

// Validate the persisted array on read — `useWidgetConfigState` keeps
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
      value: m.compute(tickets.value),
    })),
)

const pickerOpen = ref(false)

function savePicker(newIds: string[]) {
  config.metrics = newIds.slice(0, MAX_METRICS)
  pickerOpen.value = false
}
</script>

<template>
  <DashboardWidgetShell title="Queue" :loading="loading" :error="error">
    <!-- Gear in the header opens the metric picker. Lives here (not
         tied to dashboard edit mode) because picking metrics is a
         quick, in-context action — not a layout change. -->
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
