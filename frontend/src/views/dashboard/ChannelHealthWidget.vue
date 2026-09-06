<!--
Admin-only glance at inbound email channel status: enabled state,
last polled, any provider-reported error.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'
import { channelsService, type Channel, type ImapRuntimeState } from '@nosdesk/core/services/channelsService'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'
import DashboardWidgetShell from './DashboardWidgetShell.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

// `isPending` is the first-fetch signal; `isLoading` flips on every
// in-flight request including background refetches. The shell wants
// the first-only signal for its skeleton so cached content stays
// visible across remounts, see DashboardWidgetShell.
const { data, isPending, isLoading, error } = useQuery({
  key: ['channels', 'list'],
  query: () => channelsService.list(),
  // Channel config changes only on admin edits, not per navigation.
  // Hold the cache for 5 min so revisiting the dashboard serves it
  // without a background refetch; invalidateQueries still bypasses this.
  staleTime: 5 * 60_000,
})

const channels = computed<Channel[]>(() => data.value ?? [])
const isRefreshing = computed(() => isLoading.value && data.value !== undefined)
const errorMessage = computed(() =>
  error.value ? t('dashboard-channel-health-error') : null,
)

function lastError(c: Channel): string | null {
  const s = c.runtime_state as ImapRuntimeState | undefined
  return s?.last_error ?? null
}

function statusTitle(c: Channel): string {
  if (!c.enabled) return t('dashboard-channel-health-status-disabled')
  if (lastError(c)) return t('dashboard-channel-health-status-error')
  return t('dashboard-channel-health-status-healthy')
}

function polledLabel(at: string | null | undefined): string {
  return at
    ? t('dashboard-channel-health-polled', { time: formatRelativeTime(at) })
    : t('dashboard-channel-health-never-polled')
}
</script>

<template>
  <DashboardWidgetShell
    :title="t('dashboard-channel-health-title')"
    :action-label="t('dashboard-channel-health-action')"
    action-to="/admin/channels/email"
    :loading="isPending"
    :refreshing="isRefreshing"
    :error="errorMessage"
    :empty="channels.length === 0"
    :empty-title="t('dashboard-channel-health-empty-title')"
    :empty-description="t('dashboard-channel-health-empty-description')"
    min-body-height="200px"
  >
    <ul class="divide-y divide-default">
      <li
        v-for="c in channels"
        :key="c.id"
        class="px-4 py-2 flex items-start gap-2.5"
      >
        <span
          class="mt-1.5 w-2 h-2 rounded-full flex-shrink-0"
          :class="[
            !c.enabled ? 'bg-tertiary'
              : lastError(c) ? 'bg-status-error animate-pulse'
              : 'bg-status-success',
          ]"
          :title="statusTitle(c)"
          aria-hidden="true"
        />
        <div class="flex-1 min-w-0">
          <p class="text-sm text-primary truncate">{{ c.name }}</p>
          <p class="mt-0.5 text-2xs text-tertiary truncate">
            {{ c.provider }} · {{ polledLabel(c.last_polled_at) }}
          </p>
          <p v-if="lastError(c)" class="mt-0.5 text-2xs text-status-error truncate">
            {{ lastError(c) }}
          </p>
        </div>
      </li>
    </ul>
  </DashboardWidgetShell>
</template>
