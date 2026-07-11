<!--
Short list of the tickets the current user has viewed most recently.
Uses the shared `TicketRow` so it stays visually consistent with the
Assigned Tickets + Unassigned Queue widgets, same priority rail,
status icon, ID + title chrome. `RecentTicket` doesn't carry priority
or full requester details, so those columns simply aren't rendered;
the row's anatomy is preserved by the optional-field pattern.

Reads the shared `recentTickets` store (Pinia Colada, account-scoped
key) so this widget and the sidebar dedup into one request per session
and stay in sync on view / remove.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRecentTicketsStore } from '@/stores/recentTickets'
import type { RecentTicket } from '@nosdesk/core/types/ticket'
import DashboardWidgetShell from './DashboardWidgetShell.vue'
import TicketRow from '@/components/TicketRow.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

// The store exposes the first-fetch (`isLoading`) and background-
// refetch (`isRefreshing`) signals the shell wants, so cached content
// stays visible across dashboard remounts.
const store = useRecentTicketsStore()

const tickets = computed<RecentTicket[]>(() => store.recentTickets.slice(0, 5))
const errorMessage = computed(() =>
  store.error ? t('dashboard-recently-viewed-error') : null,
)
</script>

<template>
  <DashboardWidgetShell
    :title="t('dashboard-recently-viewed-title')"
    action-to="/tickets"
    :loading="store.isLoading"
    :refreshing="store.isRefreshing"
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
          :workflow-state-id="ticket.workflow_state_id"
          :timestamp="ticket.last_viewed_at"
          :to="`/tickets/${ticket.id}`"
        />
      </li>
    </ul>
  </DashboardWidgetShell>
</template>
