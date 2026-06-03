<!--
Leaderboard — top-N actor (assignee or requester) ranking by
ticket count in the window. Renders as a rank-numbered list with
inline bars sized to the leader.

Actor-uuid -> human-name resolution is a per-row backend lookup;
that arrives with the drill-through work in Wave 6. Until then,
rows render with the truncated uuid as the label.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import { useTimeRange } from '@/composables/useTimeRange'
import {
  analyticsService,
  type LeaderboardActor,
  type LeaderboardRow,
} from '@/services/analyticsService'

const props = withDefaults(
  defineProps<{
    actor: LeaderboardActor
    topN?: number
    /** Saved-view uuid for drill-through. When set, each rank row
     *  becomes a router-link to `/tickets?view=<uuid>` with the
     *  actor as a segment param. */
    viewUuid?: string
  }>(),
  {
    topN: 10,
  },
)

const fluent = useFluent()
const t = (k: string) => fluent.$t(k)

const { window: timeWindow } = useTimeRange()

const query = useQuery({
  key: () => [
    'dashboard',
    'leaderboard',
    props.actor,
    props.topN,
    timeWindow.value.from,
    timeWindow.value.to,
  ],
  query: () =>
    analyticsService.leaderboard({
      actor: props.actor,
      from: timeWindow.value.from,
      to: timeWindow.value.to,
      top_n: props.topN,
    }),
})

const rows = computed<LeaderboardRow[]>(() => query.data.value?.rows ?? [])
const loading = computed(() => query.status.value === 'pending' && rows.value.length === 0)
const hasError = computed(() => query.status.value === 'error')
const isEmpty = computed(() => !loading.value && !hasError.value && rows.value.length === 0)

const maxValue = computed(() => Math.max(1, ...rows.value.map((r) => r.value)))

function rowLabel(r: LeaderboardRow): string {
  if (!r.actor_uuid) return t('dashboard-bar-unassigned')
  // Show first 8 chars of the uuid until per-row name resolution
  // lands in Wave 6 — keeps the row identifiable for operators
  // who recognise their handles.
  return r.actor_uuid.slice(0, 8)
}

function rowLink(r: LeaderboardRow) {
  if (!props.viewUuid) return null
  return {
    path: '/tickets',
    query: {
      view: props.viewUuid,
      segment_key: props.actor,
      segment_value: r.actor_uuid ?? 'unassigned',
    },
  }
}
</script>

<template>
  <div class="flex flex-col w-full h-full p-4">
    <div v-if="loading" class="flex-1 flex items-center justify-center text-tertiary text-xs">
      {{ t('dashboard-line-chart-loading') }}
    </div>
    <div v-else-if="hasError" class="flex-1 flex items-center justify-center text-status-error text-xs">
      {{ t('dashboard-line-chart-error') }}
    </div>
    <div v-else-if="isEmpty" class="flex-1 flex items-center justify-center text-tertiary text-xs">
      {{ t('dashboard-line-chart-empty') }}
    </div>
    <ol v-else class="flex flex-col gap-1.5">
      <li v-for="(r, i) in rows" :key="r.actor_uuid ?? `unassigned-${i}`">
        <component
          :is="rowLink(r) ? 'router-link' : 'div'"
          :to="rowLink(r) ?? undefined"
          :class="[
            'grid grid-cols-[1.25rem_8rem_1fr_2.5rem] items-center gap-2 text-xs px-1 py-0.5 rounded',
            rowLink(r) ? 'transition-colors hover:bg-surface-hover' : '',
          ]"
        >
          <span class="text-tertiary text-right tabular-nums">{{ i + 1 }}</span>
          <span class="text-secondary truncate font-mono" :title="r.actor_uuid ?? ''">
            {{ rowLabel(r) }}
          </span>
          <div class="h-2 rounded-sm bg-surface-alt overflow-hidden">
            <div
              class="h-full bg-chart-2 transition-[width] duration-200"
              :style="{ width: `${(r.value / maxValue) * 100}%` }"
            />
          </div>
          <span class="text-tertiary tabular-nums text-right">{{ r.value }}</span>
        </component>
      </li>
    </ol>
  </div>
</template>
