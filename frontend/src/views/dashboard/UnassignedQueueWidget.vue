<!--
Grab-the-next-ticket panel for shared queues: the 5 oldest unassigned
open tickets. Uses the shared `TicketRow` for anatomy consistency
with other ticket-list widgets; the only widget-specific concern is
the fetch parameters (open + unassigned + oldest first).
-->
<script setup lang="ts">
import ticketService, { type Ticket } from '@/services/ticketService'
import { useAsyncResource } from '@/composables/useAsyncResource'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import TicketRow from '@/components/TicketRow.vue'

const { data: tickets, loading, error } = useAsyncResource<Ticket[]>(
  async () => {
    const res = await ticketService.getPaginatedTickets({
      page: 1,
      pageSize: 5,
      status: 'open',
      assignee: 'unassigned',
      sortField: 'created_at',
      sortDirection: 'asc',
    }, 'dashboard-unassigned-queue')
    return res.data
  },
  [],
  'Failed to load queue',
)
</script>

<template>
  <DashboardWidgetShell
    title="Unassigned Queue"
    action-to="/tickets?assignee=unassigned&status=open"
    :loading="loading"
    :error="error"
    :empty="!error && tickets.length === 0"
    empty-title="Inbox zero"
    empty-description="Nothing waiting in the queue."
    min-body-height="200px"
  >
    <ul class="divide-y divide-default">
      <li v-for="t in tickets" :key="t.id">
        <TicketRow
          :id="t.id"
          :title="t.title"
          :status="t.status"
          :priority="t.priority"
          :timestamp="t.created"
          :requester="t.requester_user"
          :to="`/tickets/${t.id}`"
        />
      </li>
    </ul>
  </DashboardWidgetShell>
</template>
