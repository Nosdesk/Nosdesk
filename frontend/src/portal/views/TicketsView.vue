<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import EmptyState from '@/components/common/EmptyState.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'

import PortalLayout from '../components/PortalLayout.vue'
import { stateTone } from '../stateTone'
import { isClosed, listMyTickets } from '../service'

const { $t: t } = useFluent()
const router = useRouter()

const tickets = useQuery({ key: ['portal', 'tickets'], query: listMyTickets })
const filter = ref<'open' | 'closed'>('open')

const visible = computed(() =>
  (tickets.data.value ?? []).filter((ticket) => isClosed(ticket) === (filter.value === 'closed')),
)
</script>

<template>
  <PortalLayout>
    <div class="flex items-center gap-3">
      <h1 class="text-xl font-semibold text-primary">{{ t('portal-nav-requests') }}</h1>
      <div class="ml-auto flex gap-1 p-0.5 rounded-lg bg-surface-alt border border-default text-sm" role="tablist">
        <button
          v-for="option in (['open', 'closed'] as const)"
          :key="option"
          type="button"
          role="tab"
          :aria-selected="filter === option"
          class="px-3 py-1 rounded-md"
          :class="filter === option ? 'bg-surface text-primary shadow-sm' : 'text-secondary hover:text-primary'"
          @click="filter = option"
        >
          {{ t(option === 'open' ? 'portal-filter-open' : 'portal-filter-closed') }}
        </button>
      </div>
    </div>

    <p v-if="tickets.error.value && !tickets.data.value" class="text-sm text-status-error">
      {{ t('portal-requests-load-failed') }}
    </p>
    <ul v-else-if="visible.length" class="flex flex-col gap-2">
      <li v-for="ticket in visible" :key="ticket.id">
        <RouterLink
          :to="`/tickets/${ticket.id}`"
          class="flex flex-col gap-1 bg-surface border border-default rounded-xl p-4 hover:border-accent transition-colors"
        >
          <div class="flex items-start gap-3">
            <span class="font-medium text-primary flex-1 min-w-0">{{ ticket.title }}</span>
            <StatusPill v-if="ticket.state" :label="ticket.state.name" :tone="stateTone(ticket.state.category)" />
          </div>
          <span class="text-xs text-tertiary">
            #{{ ticket.id }} · {{ t('portal-updated', { when: formatRelativeTime(ticket.modified) }) }}
          </span>
        </RouterLink>
      </li>
    </ul>
    <EmptyState
      v-else-if="tickets.data.value"
      icon="ticket"
      variant="card"
      :title="t(filter === 'open' ? 'portal-requests-empty-title' : 'portal-requests-empty-closed')"
      :description="filter === 'open' ? t('portal-requests-empty-description') : undefined"
      :action-label="filter === 'open' ? t('portal-nav-new') : undefined"
      @action="router.push('/tickets/new')"
    />
  </PortalLayout>
</template>
