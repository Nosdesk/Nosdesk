<!--
Short list of the tickets the current user has viewed most recently.
Uses the shared `TicketRow` so it stays visually consistent with the
Assigned Tickets + Unassigned Queue widgets — same priority rail,
status icon, ID + title chrome. `RecentTicket` doesn't carry priority
or full requester details, so those columns simply aren't rendered;
the row's anatomy is preserved by the optional-field pattern.
-->
<script setup lang="ts">
import { getRecentTickets, type RecentTicket } from '@/services/ticketService'
import { useAsyncResource } from '@/composables/useAsyncResource'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import TicketRow from '@/components/TicketRow.vue'

const { data: tickets, loading, error } = useAsyncResource<RecentTicket[]>(
  async () => (await getRecentTickets()).slice(0, 5),
  [],
  'Failed to load recently viewed',
)
</script>

<template>
  <DashboardWidgetShell
    title="Recently Viewed"
    action-to="/tickets"
    :loading="loading"
    :error="error"
    :empty="!error && tickets.length === 0"
    empty-title="Nothing here yet"
    empty-description="Tickets you open will show up here."
    min-body-height="200px"
  >
    <ul class="divide-y divide-default">
      <li v-for="t in tickets" :key="t.id">
        <TicketRow
          :id="t.id"
          :title="t.title"
          :status="t.status"
          :timestamp="t.last_viewed_at"
          :to="`/tickets/${t.id}`"
        />
      </li>
    </ul>
  </DashboardWidgetShell>
</template>
