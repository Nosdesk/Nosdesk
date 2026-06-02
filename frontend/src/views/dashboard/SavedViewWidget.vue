<!--
SavedViewWidget — the single shell every saved-view-backed widget on
the dashboard renders through (docs/dashboard-and-analytics-plan.md
decision 25). The dashboard layout references a saved view via the
synthetic widget id `saved_view:<uuid>`; the widget registry resolves
that prefix to this component and passes the uuid through.

This wave (Phase 3) lands the resolver + shell. The renderer
dispatches on the saved_view's `viz_type`:

  - 'list' (the default for any non-chart saved view): falls back
    to the "Coming soon" placeholder for now. List-on-dashboard is
    not part of this wave; pin-as-list lands when the LinkButton +
    secondary surfaces work catches up.
  - 'kpi_tile' / 'line' / 'horizontal_bar' / 'heatmap' /
    'leaderboard' / 'table': the chart components ship in Waves 4
    and 5; until then the shell renders a labelled placeholder so
    operators who pre-build chart saved views can drop them onto
    the dashboard and see they're recognised.

Data: the saved view itself is fetched via Pinia Colada useQuery so
multiple SavedViewWidget instances pinned to the same view share
one fetch, and saved-view edits in another tab pick up via the
existing saved-view SSE invalidation.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import { savedViewsService, type SavedView, type SavedViewVizType } from '@/services/savedViewsService'

const props = defineProps<{
  /** Saved view uuid this widget renders. Resolved from the
   *  synthetic widget id `saved_view:<uuid>`. */
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
const loading = computed(() => query.status.value === 'pending')
const error = computed(() => (query.error.value ? t('dashboard-saved-view-error') : null))
const isMissing = computed(() => query.status.value === 'success' && !view.value)

const vizLabel = computed(() => t(`dashboard-saved-view-viz-label-${vizType.value}`))
</script>

<template>
  <DashboardWidgetShell
    :title="title"
    :loading="loading"
    :error="error"
    :empty="isMissing"
    :flush-body="false"
    :min-body-height="'9rem'"
  >
    <div class="flex flex-col items-center justify-center gap-2 py-6 text-center">
      <p class="text-xs uppercase tracking-wide text-tertiary">{{ vizLabel }}</p>
      <p class="text-sm text-secondary">
        {{ t('dashboard-saved-view-placeholder') }}
      </p>
    </div>
  </DashboardWidgetShell>
</template>
