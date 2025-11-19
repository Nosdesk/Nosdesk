// components/RecentTickets.vue
<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { useRecentTicketsStore } from '@/stores/recentTickets'
import StatusBadge from '@/components/StatusBadge.vue'
import QuickTooltip from '@/components/QuickTooltip.vue'
import { onMounted } from 'vue'
import { formatDateTime } from '@/utils/dateUtils'

const recentTicketsStore = useRecentTicketsStore()

// Fetch recent tickets when component mounts
onMounted(async () => {
  await recentTicketsStore.fetchRecentTickets()
})
</script>

<template>
  <div class="h-full flex flex-col">
    <div class="flex-1 min-h-0 overflow-y-auto">
      <div class="flex flex-col gap-0.5 pt-1 pb-1 px-1">
        <!-- Wrap each ticket item with QuickTooltip -->
        <div v-for="ticket in recentTicketsStore.recentTickets" :key="ticket.id" class="block">
          <QuickTooltip
            :text="ticket.title"
            :details="{
              title: ticket.title,
              status: ticket.status,
              requester: ticket.requester,
              assignee: ticket.assignee,
              created: formatDateTime(ticket.created_at)
            }"
            :fullWidth="true"
          >
            <RouterLink
              :to="{
                path: `/tickets/${ticket.id}`,
                query: { fromRecent: 'true' }
              }"
              class="group block px-2 py-1 rounded-md hover:bg-surface-hover relative transition-all duration-200 border border-transparent hover:shadow-sm"
            >
              <!-- Compact layout with status badge, ID, and title in a single row -->
              <div class="flex items-center w-full gap-1">
                <!-- Status Badge -->
                <StatusBadge
                  type="status"
                  :value="ticket.status"
                  custom-classes="w-1.5 h-1.5 rounded-full flex-shrink-0 mr-1.5"
                />

                <!-- Ticket ID -->
                <span class="text-xs font-mono text-secondary whitespace-nowrap mr-2">{{ ticket.id }}</span>

                <!-- Ticket Title -->
                <span class="text-xs text-primary truncate block min-w-0 flex-1">
                  {{ ticket.title }}
                </span>
              </div>
            </RouterLink>
          </QuickTooltip>
        </div>

        <!-- Empty state - More compact -->
        <div
          v-if="recentTicketsStore.recentTickets.length === 0"
          class="px-2 py-1.5 text-xs text-secondary bg-surface text-center"
        >
          No recent tickets
        </div>
      </div>
    </div>
  </div>
</template>