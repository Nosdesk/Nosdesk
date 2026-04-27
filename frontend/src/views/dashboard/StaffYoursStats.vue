<!--
"Yours" stats: four KPIs for tickets assigned to the current user.
The actual count comes from `/api/dashboard/stats?include=yours`,
served by the dashboard coordinator (one shared request across
all stat widgets, not three independent fetches).
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useInjectedDashboardStats } from '@/composables/useDashboardStats'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import KpiRail, { type Kpi } from './KpiRail.vue'
import KpiRailSkeleton from './KpiRailSkeleton.vue'

const stats = useInjectedDashboardStats()

const counts = computed(() => stats.bundle.value?.yours ?? {
  open: 0,
  inProgress: 0,
  closed: 0,
  closedToday: 0,
  highPriority: 0,
})

const assigned = computed(() => counts.value.open + counts.value.inProgress + counts.value.closed)

const kpis = computed<Kpi[]>(() => [
  { label: 'Assigned', value: assigned.value, to: '/tickets?assignee=current', tone: 'text-primary' },
  { label: 'Open', value: counts.value.open, to: '/tickets?assignee=current&status=open', tone: 'text-status-open' },
  { label: 'In Progress', value: counts.value.inProgress, to: '/tickets?assignee=current&status=in-progress', tone: 'text-status-in-progress' },
  { label: 'Closed', value: counts.value.closed, to: '/tickets?assignee=current&status=closed', tone: 'text-tertiary' },
])

const errorMessage = computed(() => (stats.isError.value ? 'Failed to load counts' : null))
</script>

<template>
  <DashboardWidgetShell
    title="Yours"
    :loading="stats.isLoading.value"
    :refreshing="stats.isRefreshing.value"
    :error="errorMessage"
  >
    <template #skeleton><KpiRailSkeleton :count="4" /></template>
    <KpiRail :kpis="kpis" />
  </DashboardWidgetShell>
</template>
