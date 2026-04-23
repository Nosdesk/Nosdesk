<!--
"Yours" stats: four KPIs for tickets assigned to the current user.
Fetch + SSE wiring come from `useTicketStats`; layout + skeleton come
from `KpiRail` / `KpiRailSkeleton`. This file owns only the per-metric
derivation and link targets.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useTicketStats } from '@/composables/useTicketStats'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import KpiRail, { type Kpi } from './KpiRail.vue'
import KpiRailSkeleton from './KpiRailSkeleton.vue'

const auth = useAuthStore()

const { data: counts, loading, error } = useTicketStats(
  (tickets) => {
    const uuid = auth.user?.uuid ?? ''
    const mine = tickets.filter((t) => t.assignee === uuid)
    return {
      assigned: mine.length,
      open: mine.filter((t) => t.status === 'open').length,
      inProgress: mine.filter((t) => t.status === 'in-progress').length,
      closed: mine.filter((t) => t.status === 'closed').length,
    }
  },
  { assigned: 0, open: 0, inProgress: 0, closed: 0 },
  'Failed to load counts',
)

const kpis = computed<Kpi[]>(() => [
  { label: 'Assigned', value: counts.value.assigned, to: '/tickets?assignee=current', tone: 'text-primary' },
  { label: 'Open', value: counts.value.open, to: '/tickets?assignee=current&status=open', tone: 'text-status-open' },
  { label: 'In Progress', value: counts.value.inProgress, to: '/tickets?assignee=current&status=in-progress', tone: 'text-status-in-progress' },
  { label: 'Closed', value: counts.value.closed, to: '/tickets?assignee=current&status=closed', tone: 'text-tertiary' },
])
</script>

<template>
  <DashboardWidgetShell title="Yours" :loading="loading" :error="error">
    <template #skeleton><KpiRailSkeleton :count="4" /></template>
    <KpiRail :kpis="kpis" />
  </DashboardWidgetShell>
</template>
