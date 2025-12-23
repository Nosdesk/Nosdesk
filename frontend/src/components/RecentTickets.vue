<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { useRecentTicketsStore } from '@/stores/recentTickets'
import { onMounted, computed } from 'vue'
import { parseDate } from '@/utils/dateUtils'
import StatusIndicator from '@/components/common/StatusIndicator.vue'
import TicketDragPreview from '@/components/common/TicketDragPreview.vue'
import { useTicketDrag, type DraggableTicket } from '@/composables/useTicketDrag'

const recentTicketsStore = useRecentTicketsStore()
const {
  dragState,
  handleDragStart,
  handleDrag,
  handleDragEnd,
  handleTouchStart,
  handleTouchMove,
  handleTouchEnd,
  handleTouchCancel
} = useTicketDrag()

// Convert store ticket to draggable ticket format
const toDraggableTicket = (ticket: any): DraggableTicket => ({
  id: ticket.id,
  title: ticket.title,
  status: ticket.status,
  priority: ticket.priority,
  assignee: ticket.assignee || ticket.assignee_user?.display_name || null
})

// Only show loading skeleton on initial load (when we have no data yet)
const showLoading = computed(() =>
  recentTicketsStore.isLoading && recentTicketsStore.recentTickets.length === 0
)

onMounted(async () => {
  // Only fetch if we don't have data yet (prevents refetch on every mount)
  if (recentTicketsStore.recentTickets.length === 0) {
    await recentTicketsStore.fetchRecentTickets()
  }
})

// Compact relative time using app's date utilities
const relativeTime = (dateString: string | null | undefined): string => {
  if (!dateString) return ''

  // Use the app's parseDate to handle timezone correctly
  const date = parseDate(dateString)
  if (!date) return ''

  const diff = Date.now() - date.getTime()
  const mins = Math.floor(diff / 60000)
  if (mins < 1) return 'now'
  if (mins < 60) return `${mins}m`
  const hours = Math.floor(diff / 3600000)
  if (hours < 24) return `${hours}h`
  const days = Math.floor(diff / 86400000)
  if (days < 7) return `${days}d`
  return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
}
</script>

<template>
  <div class="h-full flex flex-col">
    <!-- Loading (only on initial load) -->
    <div v-if="showLoading" class="p-1 space-y-0.5">
      <div v-for="i in 8" :key="i" class="h-7 bg-surface-hover rounded animate-pulse"></div>
    </div>

    <!-- List -->
    <div v-else-if="recentTicketsStore.recentTickets.length > 0" class="flex-1 min-h-0 overflow-y-auto">
      <TransitionGroup name="ticket-list" tag="div" class="py-0.5 relative">
        <RouterLink
          v-for="ticket in recentTicketsStore.recentTickets"
          :key="ticket.id"
          :to="{ path: `/tickets/${ticket.id}`, query: { fromRecent: 'true' } }"
          class="group flex items-center gap-1.5 px-2 py-1 mx-0.5 rounded hover:bg-surface-hover transition-colors cursor-grab select-none"
          :class="{ 'opacity-50': dragState.isDragging && dragState.ticket?.id === ticket.id }"
          draggable="true"
          @dragstart="handleDragStart(toDraggableTicket(ticket), 'recent-tickets', $event)"
          @drag="handleDrag"
          @dragend="handleDragEnd"
          @touchstart="handleTouchStart(toDraggableTicket(ticket), 'recent-tickets', $event)"
          @touchmove="handleTouchMove"
          @touchend="handleTouchEnd"
          @touchcancel="handleTouchCancel"
        >
          <!-- Status indicator -->
          <StatusIndicator :status="ticket.status" size="xs" />

          <!-- ID -->
          <span class="text-xs text-secondary font-medium flex-shrink-0">#{{ ticket.id }}</span>

          <!-- Title -->
          <span class="text-xs text-primary truncate flex-1 group-hover:text-accent">
            {{ ticket.title }}
          </span>

          <!-- Time -->
          <span class="text-[10px] text-tertiary flex-shrink-0">
            {{ relativeTime(ticket.last_viewed_at || ticket.updated_at || ticket.modified || ticket.created_at || ticket.created) }}
          </span>
        </RouterLink>
      </TransitionGroup>
    </div>

    <!-- Empty -->
    <div v-else class="flex-1 flex items-center justify-center p-2">
      <p class="text-xs text-tertiary">No recent tickets</p>
    </div>

    <!-- Drag Preview (uses Teleport, doesn't affect layout) -->
    <TicketDragPreview
      v-if="dragState.isDragging && dragState.source === 'recent-tickets' && dragState.ticket && dragState.position"
      :ticket="dragState.ticket"
      :position="dragState.position"
    />
  </div>
</template>

<style scoped>
/* Thin scrollbar using theme colors */
.overflow-y-auto {
  scrollbar-width: thin;
  scrollbar-color: var(--color-text-tertiary) transparent;
}
.overflow-y-auto::-webkit-scrollbar { width: 4px; }
.overflow-y-auto::-webkit-scrollbar-track { background: transparent; }
.overflow-y-auto::-webkit-scrollbar-thumb {
  background-color: var(--color-text-tertiary);
  border-radius: 2px;
}
.overflow-y-auto::-webkit-scrollbar-thumb:hover {
  background-color: var(--color-text-secondary);
}

/* FLIP animation for list reordering */
.ticket-list-move,
.ticket-list-enter-active,
.ticket-list-leave-active {
  transition: all 0.3s ease;
}

.ticket-list-enter-from,
.ticket-list-leave-to {
  opacity: 0;
}

/* Take leaving items out of layout flow so move animations calculate correctly */
.ticket-list-leave-active {
  position: absolute;
  width: calc(100% - 4px); /* Account for mx-0.5 */
}
</style>
