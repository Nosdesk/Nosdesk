<script setup lang="ts">
/** Requests waiting for the signed-in person's approval. */
import { RouterLink } from 'vue-router'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import EmptyState from '@/components/common/EmptyState.vue'
import { formatCompactRelativeTime, formatDateTime } from '@nosdesk/core/utils/dateUtils'

import PortalAvatar from '../components/PortalAvatar.vue'
import PortalLayout from '../components/PortalLayout.vue'
import { listMyApprovals } from '../service'

const { $t: t } = useFluent()
const approvals = useQuery({ key: ['portal', 'approvals'], query: listMyApprovals })
</script>

<template>
  <PortalLayout>
    <header class="flex flex-col gap-1">
      <h1 class="text-2xl font-semibold text-primary">{{ t('portal-approvals-title') }}</h1>
      <p class="text-sm text-secondary">{{ t('portal-approvals-intro') }}</p>
    </header>
    <p v-if="approvals.error.value && !approvals.data.value" class="text-sm text-status-error">
      {{ t('portal-approvals-load-failed') }}
    </p>
    <EmptyState
      v-else-if="approvals.data.value && !approvals.data.value.length"
      icon="inbox"
      variant="card"
      :title="t('portal-approvals-empty')"
    />
    <ul
      v-else-if="approvals.data.value"
      class="flex flex-col bg-surface border border-default rounded-xl overflow-hidden divide-y divide-[var(--color-border-default)]"
    >
      <li v-for="a in approvals.data.value" :key="a.ticket_id">
        <RouterLink
          :to="`/approvals/${a.ticket_id}`"
          class="flex items-start gap-3 px-4 py-3.5 hover:bg-surface-hover focus-visible:outline-none focus-visible:bg-surface-hover transition-colors"
        >
          <PortalAvatar :name="a.requester_name ?? ''" size="sm" class="mt-0.5" />
          <span class="flex flex-col gap-1 flex-1 min-w-0">
            <span class="text-[0.9375rem] font-medium text-primary truncate">{{ a.title }}</span>
            <span class="flex flex-wrap gap-x-3 gap-y-1 text-xs text-secondary">
              <span>{{ t('portal-approvals-from', { name: a.requester_name ?? '' }) }}</span>
              <span v-if="a.request_type">{{ a.request_type }}</span>
            </span>
          </span>
          <time
            class="shrink-0 text-xs text-tertiary tabular-nums pt-0.5"
            :datetime="a.created_at"
            :title="formatDateTime(a.created_at)"
          >
            {{ formatCompactRelativeTime(a.created_at) }}
          </time>
        </RouterLink>
      </li>
    </ul>
  </PortalLayout>
</template>
