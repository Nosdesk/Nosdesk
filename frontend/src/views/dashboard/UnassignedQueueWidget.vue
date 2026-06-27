<!--
Grab-the-next-ticket panel for shared queues: the 5 oldest unassigned
open tickets. Uses the shared `TicketRow` for anatomy consistency
with other ticket-list widgets; the only widget-specific concern is
the fetch parameters (open + unassigned + oldest first).
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'
import ticketService, { type Ticket } from '@nosdesk/core/services/ticketService'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import TicketRow from '@/components/TicketRow.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

// Pinia Colada exposes both `isPending` (initial fetch only) and
// `isLoading` (every in-flight request). The widget shell wants the
// initial-only signal so cached content stays visible across remounts
// while a background refetch runs, see DashboardWidgetShell's
// `loading` vs `refreshing` props.
const { data, isPending, isLoading, error } = useQuery({
  key: ['tickets', 'unassigned-queue'],
  query: async ({ signal }) => {
    const res = await ticketService.getPaginatedTickets(
      {
        page: 1,
        pageSize: 5,
        status: 'open',
        assignee: 'unassigned',
        sortField: 'created_at',
        sortDirection: 'asc',
      },
      { signal },
    )
    return res.data
  },
})

const tickets = computed<Ticket[]>(() => data.value ?? [])
const isRefreshing = computed(() => isLoading.value && data.value !== undefined)
const errorMessage = computed(() =>
  error.value ? t('dashboard-unassigned-queue-error') : null,
)
</script>

<template>
  <DashboardWidgetShell
    :title="t('dashboard-unassigned-queue-title')"
    action-to="/tickets?assignee=unassigned&status=open"
    :loading="isPending"
    :refreshing="isRefreshing"
    :error="errorMessage"
    :empty="!errorMessage && tickets.length === 0"
    :empty-title="t('dashboard-unassigned-queue-empty-title')"
    :empty-description="t('dashboard-unassigned-queue-empty-description')"
    min-body-height="200px"
  >
    <ul class="divide-y divide-default">
      <li v-for="ticket in tickets" :key="ticket.id">
        <TicketRow
          :id="ticket.id"
          :title="ticket.title"
          :workflow-state-id="ticket.workflow_state_id"
          :priority="ticket.priority"
          :timestamp="ticket.created"
          :requester="ticket.requester_user"
          :to="`/tickets/${ticket.id}`"
        />
      </li>
    </ul>
  </DashboardWidgetShell>
</template>
