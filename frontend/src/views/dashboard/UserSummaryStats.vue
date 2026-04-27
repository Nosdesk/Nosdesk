<!--
"Summary" stats for end users: counts of the user's own requests by
status. Structurally identical to `StaffYoursStats` but reads the
`summary` slice (requester-scoped on the backend) and uses end-user
labels. Counts come from the dashboard coordinator,
`useInjectedDashboardStats()`, one shared request across all stat
widgets.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useInjectedDashboardStats } from '@/composables/useDashboardStats'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import KpiRail, { type Kpi } from './KpiRail.vue'
import KpiRailSkeleton from './KpiRailSkeleton.vue'

const stats = useInjectedDashboardStats()

const counts = computed(() => stats.bundle.value?.summary ?? {
  open: 0,
  inProgress: 0,
  closed: 0,
  closedToday: 0,
  highPriority: 0,
})

const total = computed(() => counts.value.open + counts.value.inProgress + counts.value.closed)

const kpis = computed<Kpi[]>(() => [
  { label: 'Requests', value: total.value, to: '/tickets?requester=current', tone: 'text-primary' },
  { label: 'Open', value: counts.value.open, to: '/tickets?requester=current&status=open', tone: 'text-status-open' },
  { label: 'In Progress', value: counts.value.inProgress, to: '/tickets?requester=current&status=in-progress', tone: 'text-status-in-progress' },
  { label: 'Resolved', value: counts.value.closed, to: '/tickets?requester=current&status=closed', tone: 'text-status-closed' },
])

const errorMessage = computed(() => (stats.isError.value ? 'Failed to load summary' : null))
</script>

<template>
  <DashboardWidgetShell
    title="Summary"
    :loading="stats.isLoading.value"
    :refreshing="stats.isRefreshing.value"
    :error="errorMessage"
  >
    <template #skeleton><KpiRailSkeleton :count="4" /></template>
    <KpiRail :kpis="kpis" />
  </DashboardWidgetShell>
</template>
