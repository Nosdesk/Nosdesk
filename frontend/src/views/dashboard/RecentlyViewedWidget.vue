<!--
Short list of the tickets the current user has viewed most recently.
Uses the shared `TicketRow` so it stays visually consistent with the
Assigned Tickets + Unassigned Queue widgets, same priority rail,
status icon, ID + title chrome. `RecentTicket` doesn't carry priority
or full requester details, so those columns simply aren't rendered;
the row's anatomy is preserved by the optional-field pattern.

The query key `['tickets', 'recent']` is shared with the sidebar's
recent-tickets store. Once that store is migrated to Pinia Colada
they'll dedup into a single network request per session.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'
import { getRecentTickets, type RecentTicket } from '@/services/ticketService'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import TicketRow from '@/components/TicketRow.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

// `isPending` = first-ever fetch, `isLoading` = any in-flight
// request. The shell wants the first-only signal for `loading` so
// dashboard remounts don't blank cached content while a background
// refetch runs, see DashboardWidgetShell.
const { data, isPending, isLoading, error } = useQuery({
  key: ['tickets', 'recent'],
  query: () => getRecentTickets(),
})

const tickets = computed<RecentTicket[]>(() => (data.value ?? []).slice(0, 5))
const isRefreshing = computed(() => isLoading.value && data.value !== undefined)
const errorMessage = computed(() =>
  error.value ? t('dashboard-recently-viewed-error') : null,
)
</script>

<template>
  <DashboardWidgetShell
    :title="t('dashboard-recently-viewed-title')"
    action-to="/tickets"
    :loading="isPending"
    :refreshing="isRefreshing"
    :error="errorMessage"
    :empty="!errorMessage && tickets.length === 0"
    :empty-title="t('dashboard-recently-viewed-empty-title')"
    :empty-description="t('dashboard-recently-viewed-empty-description')"
    min-body-height="200px"
  >
    <ul class="divide-y divide-default">
      <li v-for="ticket in tickets" :key="ticket.id">
        <TicketRow
          :id="ticket.id"
          :title="ticket.title"
          :status="ticket.status"
          :timestamp="ticket.last_viewed_at"
          :to="`/tickets/${ticket.id}`"
        />
      </li>
    </ul>
  </DashboardWidgetShell>
</template>
