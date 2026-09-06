<!--
  KnowledgeGapsWidget: top-of-queue knowledge gaps on the
  dashboard.

  Reads from the shared `useInjectedDashboardStats` coordinator —
  one /api/dashboard/stats?include=knowledge_gaps round-trip
  serves this widget alongside the other stat widgets, no separate
  fetch. The deeper queue lives at /documentation/gaps.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { RouterLink } from 'vue-router'
import { useInjectedDashboardStats } from '@/composables/useDashboardStats'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import Icon from '@/components/common/Icon.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const stats = useInjectedDashboardStats()

const knowledgeGaps = computed(() => stats.bundle.value?.knowledgeGaps)
const items = computed(() => knowledgeGaps.value?.top ?? [])
const total = computed(() => knowledgeGaps.value?.total ?? 0)

const titleText = computed(() =>
  total.value > 0
    ? t('dashboard-knowledge-gaps-title-with-count', { count: total.value })
    : t('dashboard-knowledge-gaps-title'),
)
const errorText = computed(() => (stats.isError.value ? t('dashboard-knowledge-gaps-error') : null))

/** Failed-search gaps use a distinct title prefix so we can tell
 *  signal-type from the summary row alone. */
function isSearchGap(gapTitle: string): boolean {
  return gapTitle.startsWith('Customers searched:')
}
function impactBadge(gap: { title: string; impactScore: number }): string {
  return isSearchGap(gap.title)
    ? t('dashboard-knowledge-gaps-impact-searches', { count: gap.impactScore })
    : t('dashboard-knowledge-gaps-impact-tickets', { count: gap.impactScore })
}
function impactTooltip(gap: { title: string; impactScore: number }): string {
  return isSearchGap(gap.title)
    ? t('dashboard-knowledge-gaps-impact-tooltip-searches', { count: gap.impactScore })
    : t('dashboard-knowledge-gaps-impact-tooltip-tickets', { count: gap.impactScore })
}
function signalCount(count: number): string {
  return t('dashboard-knowledge-gaps-signal-count', { count })
}
</script>

<template>
  <DashboardWidgetShell
    :title="titleText"
    action-to="/documentation/gaps"
    :action-label="t('dashboard-knowledge-gaps-action')"
    :loading="stats.isLoading.value"
    :refreshing="stats.isRefreshing.value"
    :error="errorText"
    :empty="!stats.isError.value && items.length === 0"
    :empty-title="t('dashboard-knowledge-gaps-empty-title')"
    :empty-description="t('dashboard-knowledge-gaps-empty-description')"
    min-body-height="200px"
  >
    <ul class="divide-y divide-default">
      <li v-for="item in items" :key="item.id">
        <RouterLink
          :to="`/documentation/gaps/${item.id}`"
          class="flex items-start gap-3 px-3 py-2 hover:bg-surface-hover transition-colors"
        >
          <Icon name="warning" class="text-amber-500 flex-shrink-0 mt-0.5" />
          <div class="flex-1 min-w-0">
            <p class="text-sm text-primary truncate">{{ item.title }}</p>
            <div class="text-2xs text-tertiary mt-0.5 flex items-center gap-2">
              <span>{{ signalCount(item.evidenceCount) }}</span>
              <span v-if="item.lastEvidenceAt" class="text-subtle">&middot;</span>
              <span v-if="item.lastEvidenceAt">
                {{ formatRelativeTime(item.lastEvidenceAt) }}
              </span>
            </div>
          </div>
          <span
            class="flex-shrink-0 text-3xs px-1.5 py-0.5 rounded bg-surface-alt text-tertiary"
            :title="impactTooltip(item)"
          >
            {{ impactBadge(item) }}
          </span>
        </RouterLink>
      </li>
    </ul>
  </DashboardWidgetShell>
</template>
