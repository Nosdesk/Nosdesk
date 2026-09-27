<script setup lang="ts">
/** Requests waiting for the signed-in person's approval. */
import { RouterLink } from 'vue-router'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import EmptyState from '@/components/common/EmptyState.vue'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'

import PortalLayout from '../components/PortalLayout.vue'
import { listMyApprovals } from '../service'

const { $t: t } = useFluent()
const approvals = useQuery({ key: ['portal', 'approvals'], query: listMyApprovals })
</script>

<template>
  <PortalLayout>
    <h1 class="text-xl font-semibold text-primary">{{ t('portal-approvals-title') }}</h1>
    <p v-if="approvals.error.value && !approvals.data.value" class="text-sm text-status-error">
      {{ t('portal-approvals-load-failed') }}
    </p>
    <EmptyState
      v-else-if="approvals.data.value && !approvals.data.value.length"
      icon="inbox"
      :title="t('portal-approvals-empty')"
    />
    <ul v-else-if="approvals.data.value" class="flex flex-col divide-y divide-default bg-surface border border-default rounded-xl">
      <li v-for="a in approvals.data.value" :key="a.ticket_id">
        <RouterLink :to="`/approvals/${a.ticket_id}`" class="flex flex-col gap-0.5 px-4 py-3 hover:bg-surface-hover">
          <span class="text-sm font-medium text-primary">{{ a.title }}</span>
          <span class="text-xs text-secondary">
            {{ t('portal-approvals-from', { name: a.requester_name ?? '' }) }}
            <template v-if="a.request_type"> · {{ a.request_type }}</template>
            · {{ formatRelativeTime(a.created_at) }}
          </span>
        </RouterLink>
      </li>
    </ul>
  </PortalLayout>
</template>
