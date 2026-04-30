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
import { RouterLink } from 'vue-router'
import { useInjectedDashboardStats } from '@/composables/useDashboardStats'
import { formatRelativeTime } from '@/utils/dateUtils'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import Icon from '@/components/common/Icon.vue'

const stats = useInjectedDashboardStats()

const knowledgeGaps = computed(() => stats.bundle.value?.knowledgeGaps)
const items = computed(() => knowledgeGaps.value?.top ?? [])
const total = computed(() => knowledgeGaps.value?.total ?? 0)
</script>

<template>
  <DashboardWidgetShell
    :title="total > 0 ? `Knowledge gaps (${total})` : 'Knowledge gaps'"
    action-to="/documentation/gaps"
    action-label="View queue"
    :loading="stats.isLoading.value"
    :refreshing="stats.isRefreshing.value"
    :error="stats.isError.value ? 'Failed to load gaps' : null"
    :empty="!stats.isError.value && items.length === 0"
    empty-title="No open gaps"
    empty-description="Tickets flagged for documentation will appear here."
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
            <div class="text-[11px] text-tertiary mt-0.5 flex items-center gap-2">
              <span>{{ item.evidenceCount }} signal{{ item.evidenceCount === 1 ? '' : 's' }}</span>
              <span v-if="item.lastEvidenceAt" class="text-subtle">&middot;</span>
              <span v-if="item.lastEvidenceAt">
                {{ formatRelativeTime(item.lastEvidenceAt) }}
              </span>
            </div>
          </div>
          <span
            class="flex-shrink-0 text-[10px] px-1.5 py-0.5 rounded bg-surface-alt text-tertiary"
            :title="`${item.impactScore} ticket${item.impactScore === 1 ? '' : 's'} this doc would cover`"
          >
            {{ item.impactScore }}&nbsp;ticket{{ item.impactScore === 1 ? '' : 's' }}
          </span>
        </RouterLink>
      </li>
    </ul>
  </DashboardWidgetShell>
</template>
