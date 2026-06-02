<!--
SavedViewWidget — the single shell every saved-view-backed widget on
the dashboard renders through (docs/dashboard-and-analytics-plan.md
decision 25). The dashboard layout references a saved view via the
synthetic widget id `saved_view:<uuid>`; the widget registry resolves
that prefix to this component and passes the uuid through.

The renderer dispatches on the saved_view's `viz_type`:

  - `list` (the default for any non-chart saved view): renders a
    "coming soon" placeholder. List-on-dashboard isn't part of the
    analytics overhaul; pin-as-list lands when the LinkButton +
    secondary surfaces work catches up.
  - `kpi_tile` -> KpiTile (Wave 4)
  - `line` -> LineChart (Wave 4)
  - `horizontal_bar` / `heatmap` / `leaderboard` / `table`: render
    the same labelled placeholder pending Waves 5+, so operators
    who pre-build those saved views can still pin them.

Data: the saved view itself is fetched via Pinia Colada useQuery so
multiple SavedViewWidget instances pinned to the same view share
one fetch, and saved-view edits in another tab pick up via the
existing saved-view SSE invalidation. The chart components then
fetch their own data scoped through useTimeRange so a time-range
change at the dashboard level re-runs the chart's query without
the saved-view row being re-fetched.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import KpiTile from './charts/KpiTile.vue'
import LineChart from './charts/LineChart.vue'
import HorizontalBar from './charts/HorizontalBar.vue'
import Heatmap from './charts/Heatmap.vue'
import Leaderboard from './charts/Leaderboard.vue'
import { savedViewsService, type SavedView, type SavedViewVizType } from '@/services/savedViewsService'
import type {
  BreakdownGroupBy,
  KpiMetric,
  LeaderboardActor,
  TsMeasure,
  TsTimeField,
} from '@/services/analyticsService'

const props = defineProps<{
  viewUuid: string
}>()

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const query = useQuery({
  key: () => ['saved-view', props.viewUuid],
  query: () => savedViewsService.get(props.viewUuid),
  enabled: () => !!props.viewUuid,
})

const view = computed<SavedView | undefined>(() => query.data.value)
const title = computed(() => view.value?.name ?? t('dashboard-saved-view-loading-title'))
const vizType = computed<SavedViewVizType>(() => view.value?.viz_type ?? 'list')
const vizConfig = computed<Record<string, unknown>>(() => view.value?.viz_config ?? {})
const loading = computed(() => query.status.value === 'pending')
const error = computed(() => (query.error.value ? t('dashboard-saved-view-error') : null))
const isMissing = computed(() => query.status.value === 'success' && !view.value)

const vizLabel = computed(() => t(`dashboard-saved-view-viz-label-${vizType.value}`))

const kpiProps = computed<{ metric: KpiMetric } | null>(() => {
  if (vizType.value !== 'kpi_tile') return null
  const metric = vizConfig.value.metric
  if (
    metric === 'tickets_created'
    || metric === 'tickets_resolved'
    || metric === 'tickets_open'
  ) {
    return { metric }
  }
  return null
})

const lineProps = computed<{ measure: TsMeasure; timeField: TsTimeField } | null>(() => {
  if (vizType.value !== 'line') return null
  const m = vizConfig.value.measures
  const measure: TsMeasure = Array.isArray(m) && m[0] === 'count' ? 'count' : 'count'
  const field = vizConfig.value.time_field
  const timeField: TsTimeField =
    field === 'closed_at' || field === 'resolved_at' ? (field as TsTimeField) : 'created_at'
  return { measure, timeField }
})

const barProps = computed<{ groupBy: BreakdownGroupBy; topN: number } | null>(() => {
  if (vizType.value !== 'horizontal_bar') return null
  const g = vizConfig.value.group_by
  let groupBy: BreakdownGroupBy
  if (g === 'priority') groupBy = 'priority'
  else if (g === 'category_id' || g === 'category') groupBy = 'category'
  else if (g === 'assignee_uuid' || g === 'assignee') groupBy = 'assignee'
  else return null
  const rawN = vizConfig.value.top_n
  const topN = typeof rawN === 'number' && rawN > 0 ? Math.min(rawN, 50) : 10
  return { groupBy, topN }
})

const leaderboardProps = computed<{ actor: LeaderboardActor; topN: number } | null>(() => {
  if (vizType.value !== 'leaderboard') return null
  const a = vizConfig.value.actor_field ?? vizConfig.value.actor
  let actor: LeaderboardActor
  if (a === 'assignee' || a === 'assignee_uuid') actor = 'assignee'
  else if (a === 'requester' || a === 'requester_uuid') actor = 'requester'
  else return null
  const rawN = vizConfig.value.top_n
  const topN = typeof rawN === 'number' && rawN > 0 ? Math.min(rawN, 50) : 10
  return { actor, topN }
})

const isHeatmap = computed(() => vizType.value === 'heatmap')

const supportsRenderer = computed(() =>
  ['kpi_tile', 'line', 'horizontal_bar', 'heatmap', 'leaderboard'].includes(vizType.value),
)
</script>

<template>
  <DashboardWidgetShell
    :title="title"
    :loading="loading"
    :error="error"
    :empty="isMissing"
    :flush-body="vizType === 'kpi_tile'"
    :min-body-height="'9rem'"
  >
    <KpiTile
      v-if="kpiProps"
      :metric="kpiProps.metric"
      :view-uuid="viewUuid"
    />
    <LineChart
      v-else-if="lineProps"
      :measure="lineProps.measure"
      :time-field="lineProps.timeField"
      :view-uuid="viewUuid"
    />
    <HorizontalBar
      v-else-if="barProps"
      :group-by="barProps.groupBy"
      :top-n="barProps.topN"
      :view-uuid="viewUuid"
    />
    <Heatmap v-else-if="isHeatmap" :view-uuid="viewUuid" />
    <Leaderboard
      v-else-if="leaderboardProps"
      :actor="leaderboardProps.actor"
      :top-n="leaderboardProps.topN"
      :view-uuid="viewUuid"
    />
    <div
      v-else
      class="flex flex-col items-center justify-center gap-2 py-6 text-center"
    >
      <p class="text-xs uppercase tracking-wide text-tertiary">{{ vizLabel }}</p>
      <p v-if="supportsRenderer === false" class="text-sm text-secondary">
        {{ t('dashboard-saved-view-placeholder') }}
      </p>
      <p v-else class="text-sm text-status-error">
        {{ t('dashboard-saved-view-misconfigured') }}
      </p>
    </div>
  </DashboardWidgetShell>
</template>
