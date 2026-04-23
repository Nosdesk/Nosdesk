<!--
Admin-only glance at inbound email channel status — enabled state,
last polled, any provider-reported error.
-->
<script setup lang="ts">
import { channelsService, type Channel, type ImapRuntimeState } from '@/services/channelsService'
import { formatRelativeTime } from '@/utils/dateUtils'
import { useAsyncResource } from '@/composables/useAsyncResource'
import DashboardWidgetShell from './DashboardWidgetShell.vue'

const { data: channels, loading, error } = useAsyncResource<Channel[]>(
  () => channelsService.list(),
  [],
  'Failed to load channels',
)

function lastError(c: Channel): string | null {
  const s = c.runtime_state as ImapRuntimeState | undefined
  return s?.last_error ?? null
}
</script>

<template>
  <DashboardWidgetShell
    title="Channel Health"
    action-label="Manage"
    action-to="/admin/channels/email"
    :loading="loading"
    :error="error"
    :empty="channels.length === 0"
    empty-title="No channels configured"
    empty-description="Add an email channel to ingest tickets."
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
          :title="!c.enabled ? 'Disabled' : lastError(c) ? 'Error' : 'Healthy'"
          aria-hidden="true"
        />
        <div class="flex-1 min-w-0">
          <p class="text-sm text-primary truncate">{{ c.name }}</p>
          <p class="mt-0.5 text-[11px] text-tertiary truncate">
            {{ c.provider }} · {{ c.last_polled_at ? `polled ${formatRelativeTime(c.last_polled_at)}` : 'never polled' }}
          </p>
          <p v-if="lastError(c)" class="mt-0.5 text-[11px] text-status-error truncate">
            {{ lastError(c) }}
          </p>
        </div>
      </li>
    </ul>
  </DashboardWidgetShell>
</template>
