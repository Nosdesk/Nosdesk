<script setup lang="ts">
/**
 * The requester's requests, one row each like the agent app's list: where it
 * stands (the status glyph and state), whether the team has replied since
 * they last looked, and when it last moved.
 */
import { computed, ref } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import EmptyState from '@/components/common/EmptyState.vue'
import SearchInput from '@/components/common/SearchInput.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import TicketStatusIcon from '@/components/TicketStatusIcon.vue'
import { formatCompactRelativeTime, formatDateTime } from '@nosdesk/core/utils/dateUtils'

import PortalLayout from '../components/PortalLayout.vue'
import { isClosed, listMyTickets, type PortalTicket } from '../service'

const { $t: t } = useFluent()
const router = useRouter()

const tickets = useQuery({ key: ['portal', 'tickets'], query: listMyTickets })
const filter = ref<'open' | 'closed'>('open')
const search = ref('')

const all = computed(() => tickets.data.value ?? [])
const openCount = computed(() => all.value.filter((ticket) => !isClosed(ticket)).length)
const visible = computed(() => {
  const needle = search.value.trim().toLowerCase().replace(/^#/, '')
  return all.value
    .filter((ticket) => isClosed(ticket) === (filter.value === 'closed'))
    .filter(
      (ticket) =>
        !needle || ticket.title.toLowerCase().includes(needle) || String(ticket.number) === needle,
    )
})

// The API's naive timestamps are UTC.
const ms = (at: string): number => Date.parse(/Z|[+-]\d\d:\d\d$/.test(at) ? at : `${at}Z`)
/** What moved last: a reply from the team, or the request itself. */
function lastMove(ticket: PortalTicket): string {
  const reply = ticket.last_reply_at
  return reply && ms(reply) > ms(ticket.modified) ? reply : ticket.modified
}
</script>

<template>
  <PortalLayout>
    <div class="flex flex-wrap items-center gap-3">
      <h1 class="text-2xl font-semibold text-primary mr-auto">{{ t('portal-nav-requests') }}</h1>
      <SearchInput
        v-if="all.length > 8"
        v-model="search"
        class="w-full sm:w-56 order-last sm:order-none"
        :placeholder="t('portal-requests-search')"
      />
      <SegmentedControl
        v-model="filter"
        size="sm"
        :aria-label="t('portal-requests-filter')"
        :options="[
          { value: 'open', label: t('portal-filter-open-count', { count: openCount }) },
          { value: 'closed', label: t('portal-filter-closed') },
        ]"
      />
    </div>

    <p v-if="tickets.error.value && !tickets.data.value" class="text-sm text-status-error">
      {{ t('portal-requests-load-failed') }}
    </p>

    <ul
      v-else-if="visible.length"
      class="flex flex-col bg-surface border border-default rounded-xl overflow-hidden divide-y divide-[var(--color-border-default)]"
    >
      <li v-for="ticket in visible" :key="ticket.id">
        <RouterLink
          :to="`/tickets/${ticket.number}`"
          class="group flex items-start gap-3 px-4 py-3.5 hover:bg-surface-hover focus-visible:outline-none focus-visible:bg-surface-hover transition-colors"
        >
          <TicketStatusIcon
            :category="ticket.state?.category"
            :title="ticket.state?.name"
            class="w-4 h-4 mt-[3px]"
          />
          <span class="flex flex-col gap-1 flex-1 min-w-0">
            <span class="flex items-center gap-2 min-w-0">
              <span
                class="truncate text-[0.9375rem]"
                :class="ticket.unread_reply ? 'font-semibold text-primary' : 'font-medium text-primary'"
              >
                {{ ticket.title }}
              </span>
              <span
                v-if="ticket.unread_reply"
                class="shrink-0 inline-flex items-center gap-1.5 text-xs font-medium text-accent"
              >
                <span class="w-1.5 h-1.5 rounded-full bg-accent" aria-hidden="true" />
                {{ t('portal-requests-new-reply') }}
              </span>
            </span>
            <span class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-secondary">
              <span class="tabular-nums text-tertiary">#{{ ticket.number }}</span>
              <span>{{ ticket.state?.name }}</span>
              <span v-if="ticket.approval_state === 'pending'" class="text-status-warning">
                {{ t('portal-requests-awaiting-approval') }}
              </span>
              <span v-if="ticket.requested_by">{{ t('portal-requested-by', { name: ticket.requested_by }) }}</span>
            </span>
          </span>
          <time
            class="shrink-0 text-xs text-tertiary tabular-nums pt-0.5"
            :datetime="lastMove(ticket)"
            :title="formatDateTime(lastMove(ticket))"
          >
            {{ formatCompactRelativeTime(lastMove(ticket)) }}
          </time>
        </RouterLink>
      </li>
    </ul>

    <EmptyState
      v-else-if="tickets.data.value && search.trim()"
      icon="search"
      variant="card"
      :title="t('portal-requests-no-match')"
    />
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
