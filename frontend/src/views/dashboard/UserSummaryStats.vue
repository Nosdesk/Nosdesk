<!--
"Summary" stats for end users: counts of the user's own requests by
status. Structurally identical to `StaffYoursStats` but filters on
`requester` instead of `assignee` and uses end-user-facing labels.
Shared plumbing lives in `useTicketStats` and `KpiRail`.
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
    const mine = tickets.filter((t) => t.requester === uuid)
    return {
      total: mine.length,
      open: mine.filter((t) => t.status === 'open').length,
      inProgress: mine.filter((t) => t.status === 'in-progress').length,
      closed: mine.filter((t) => t.status === 'closed').length,
    }
  },
  { total: 0, open: 0, inProgress: 0, closed: 0 },
  'Failed to load summary',
)

const kpis = computed<Kpi[]>(() => [
  { label: 'Requests', value: counts.value.total, to: '/tickets?requester=current', tone: 'text-primary' },
  { label: 'Open', value: counts.value.open, to: '/tickets?requester=current&status=open', tone: 'text-status-open' },
  { label: 'In Progress', value: counts.value.inProgress, to: '/tickets?requester=current&status=in-progress', tone: 'text-status-in-progress' },
  { label: 'Resolved', value: counts.value.closed, to: '/tickets?requester=current&status=closed', tone: 'text-status-closed' },
])
</script>

<template>
  <DashboardWidgetShell title="Summary" :loading="loading" :error="error">
    <template #skeleton><KpiRailSkeleton :count="4" /></template>
    <KpiRail :kpis="kpis" />
  </DashboardWidgetShell>
</template>
