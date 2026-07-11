<!--
Workspace SLA health at a glance: total tickets covered by a policy
and how they break down by pill state. Uses the shared KpiRail
primitive so the dashboard reads consistently. Reuses the same
backend scan as the per-policy admin counts via the shared
services::sla::scan_open_ticket_buckets helper, so the numbers here
always agree with what the policy list shows.

Refreshes on a 30s tick (same cadence as the SLA admin counts +
backend breach-detection job) so the breach signal stays current
without a page reload.
-->
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'
import { slaService } from '@nosdesk/core/services/slaService'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import KpiRail, { type Kpi } from './KpiRail.vue'
import KpiRailSkeleton from './KpiRailSkeleton.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const SLA_SUMMARY_KEY = ['sla', 'workspace-summary'] as const
const SLA_SUMMARY_REFRESH_MS = 30_000

const { data, isPending, isLoading, error, refetch } = useQuery({
  key: SLA_SUMMARY_KEY,
  query: () => slaService.getWorkspaceSummary(),
})

const counts = computed(
  () => data.value ?? { total: 0, on_track: 0, at_risk: 0, breached: 0, paused: 0 },
)
const isRefreshing = computed(() => isLoading.value && data.value !== undefined)
const errorMessage = computed(() => (error.value ? t('dashboard-sla-health-error') : null))

// Keep the snapshot fresh on the same cadence as the SLA admin
// counts + the breach-detection job (breaches are time-driven and
// emit no ticket mutation, so this poll is the only refresh channel).
// The shell handles the background-refetch visual so the existing
// numbers stay readable during the in-flight request. Ticks are
// skipped while the tab is hidden, and one catch-up refetch fires
// when it becomes visible again, so a backgrounded dashboard stops
// polling every 30s.
let timer: ReturnType<typeof setInterval> | undefined
function onVisible() {
  if (document.visibilityState === 'visible') refetch()
}
onMounted(() => {
  timer = setInterval(() => {
    if (document.visibilityState === 'visible') refetch()
  }, SLA_SUMMARY_REFRESH_MS)
  document.addEventListener('visibilitychange', onVisible)
})
onBeforeUnmount(() => {
  if (timer) clearInterval(timer)
  document.removeEventListener('visibilitychange', onVisible)
})

// Four KPIs that read top-to-bottom in priority order: how many
// tickets are tracked, how many are breaching, how many about to,
// how many are paused. On-track is the implicit remainder so the
// "tracked" cell is the at-a-glance health number.
//
// The drill-down links go to the ticket list pre-filtered by SLA
// state. These query params are accepted by the existing tickets
// list filter chain; if a state isn't filterable yet, the link
// degrades to the unfiltered list, which is still useful.
const kpis = computed<Kpi[]>(() => [
  {
    id: 'tracked',
    label: t('dashboard-sla-health-tracked'),
    value: counts.value.total,
    to: '/tickets',
    tone: 'text-primary',
  },
  {
    id: 'breached',
    label: t('dashboard-sla-health-breached'),
    value: counts.value.breached,
    to: '/tickets?sla=breached',
    tone: counts.value.breached > 0 ? 'text-status-error' : 'text-tertiary',
  },
  {
    id: 'at-risk',
    label: t('dashboard-sla-health-at-risk'),
    value: counts.value.at_risk,
    to: '/tickets?sla=at-risk',
    tone: counts.value.at_risk > 0 ? 'text-status-warning' : 'text-tertiary',
  },
  {
    id: 'paused',
    label: t('dashboard-sla-health-paused'),
    value: counts.value.paused,
    to: '/tickets?sla=paused',
    tone: 'text-tertiary',
  },
])

const isEmpty = computed(() => counts.value.total === 0)
</script>

<template>
  <DashboardWidgetShell
    :title="t('dashboard-sla-health-title')"
    :action-label="t('dashboard-sla-health-action')"
    action-to="/admin/sla"
    :loading="isPending"
    :refreshing="isRefreshing"
    :error="errorMessage"
    :empty="isEmpty"
    :empty-title="t('dashboard-sla-health-empty-title')"
    :empty-description="t('dashboard-sla-health-empty-description')"
  >
    <template #skeleton><KpiRailSkeleton :count="4" /></template>
    <KpiRail :kpis="kpis" />
  </DashboardWidgetShell>
</template>
