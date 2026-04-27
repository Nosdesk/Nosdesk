<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { useRecentTicketsStore } from '@/stores/recentTickets'
import { ref, onMounted, computed } from 'vue'
import { formatCompactRelativeTime } from '@/utils/dateUtils'
import StatusIndicator from '@/components/common/StatusIndicator.vue'
import TicketDragPreview from '@/components/common/TicketDragPreview.vue'
import ContextMenu from '@/components/common/ContextMenu.vue'
import type { MenuItem } from '@/components/common/ContextMenu.vue'
import { useTicketDrag, type DraggableTicket } from '@/composables/useTicketDrag'
import { useClipboard } from '@/composables/useClipboard'
import type { RecentTicket } from '@/types/ticket'

const recentTicketsStore = useRecentTicketsStore()
const {
  dragState,
  handleDragStart: baseDragStart,
  handleDrag: baseDrag,
  handleDragEnd,
  handleTouchStart,
  handleTouchMove,
  handleTouchEnd,
  handleTouchCancel
} = useTicketDrag()

// Context menu state
const { copy } = useClipboard()
const contextMenuTicket = ref<RecentTicket | null>(null)
const contextMenuPos = ref({ x: 0, y: 0 })
const showContextMenu = ref(false)

const ticketContextMenuItems: MenuItem[] = [
  { id: 'open-new-tab', label: 'Open in new tab', icon: 'M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25' },
  { id: 'copy-link', label: 'Copy link', icon: 'M13.19 8.688a4.5 4.5 0 011.242 7.244l-4.5 4.5a4.5 4.5 0 01-6.364-6.364l1.757-1.757m13.35-.622l1.757-1.757a4.5 4.5 0 00-6.364-6.364l-4.5 4.5a4.5 4.5 0 001.242 7.244' },
  { id: 'remove-recent', label: 'Remove from recent', icon: 'M6 18L18 6M6 6l12 12', danger: true, divider: true },
]

const handleTicketContextMenu = (ticket: RecentTicket, event: MouseEvent) => {
  event.preventDefault()
  event.stopPropagation()
  contextMenuTicket.value = ticket
  contextMenuPos.value = { x: event.clientX, y: event.clientY }
  showContextMenu.value = true
}

const handleTicketContextMenuSelect = async (actionId: string) => {
  const ticket = contextMenuTicket.value
  if (!ticket) return

  const ticketUrl = `/tickets/${ticket.id}`

  switch (actionId) {
    case 'open-new-tab':
      window.open(ticketUrl, '_blank')
      break

    case 'copy-link':
      await copy(`${window.location.origin}${ticketUrl}`)
      break

    case 'remove-recent':
      await recentTicketsStore.removeTicket(ticket.id)
      break
  }
}

// Local drag state for reordering
const draggedIndex = ref<number | null>(null)
const dropTargetIndex = ref<number | null>(null)
const isOutsideList = ref(false)
const listContainerRef = ref<HTMLElement | null>(null)

// Convert store ticket to draggable ticket format
const toDraggableTicket = (ticket: RecentTicket): DraggableTicket => ({
  id: ticket.id,
  title: ticket.title,
  status: ticket.status,
})

// `isLoading` from the store is already the first-fetch-only signal,
// so binding it directly skips the skeleton on background refetches.
const showLoading = computed(() => recentTicketsStore.isLoading)

// Custom drag start - track the dragged index
const handleDragStart = (ticket: RecentTicket, index: number, event: DragEvent) => {
  draggedIndex.value = index
  isOutsideList.value = false
  baseDragStart(toDraggableTicket(ticket), 'recent-tickets', event)
}

// Custom drag handler - check if we're inside or outside the list
const handleDrag = (event: DragEvent) => {
  baseDrag(event)

  if (listContainerRef.value && event.clientX && event.clientY) {
    const rect = listContainerRef.value.getBoundingClientRect()
    const padding = 20 // Allow some tolerance
    isOutsideList.value =
      event.clientX < rect.left - padding ||
      event.clientX > rect.right + padding ||
      event.clientY < rect.top - padding ||
      event.clientY > rect.bottom + padding
  }
}

// Handle drag over items to determine drop position
const handleDragOver = (index: number, event: DragEvent) => {
  event.preventDefault()
  if (draggedIndex.value === null) return
  if (draggedIndex.value === index) {
    dropTargetIndex.value = null
    return
  }

  const target = event.currentTarget as HTMLElement
  const rect = target.getBoundingClientRect()
  const midpoint = rect.top + rect.height / 2

  // Determine if dropping above or below the item
  if (event.clientY < midpoint) {
    dropTargetIndex.value = index
  } else {
    dropTargetIndex.value = index + 1
  }
}

// Handle drag leave
const handleDragLeave = () => {
  // Don't clear immediately - let dragover handle it
}

// Handle drop - reorder if inside list
const handleDrop = (event: DragEvent) => {
  event.preventDefault()

  if (draggedIndex.value !== null && dropTargetIndex.value !== null && !isOutsideList.value) {
    let toIndex = dropTargetIndex.value
    // Adjust index if dropping after the dragged item
    if (toIndex > draggedIndex.value) {
      toIndex -= 1
    }
    recentTicketsStore.reorderTickets(draggedIndex.value, toIndex)
  }

  resetDragState()
}

// Reset local drag state
const resetDragState = () => {
  draggedIndex.value = null
  dropTargetIndex.value = null
  isOutsideList.value = false
}

// Wrap drag end to reset local state
const handleLocalDragEnd = () => {
  resetDragState()
  handleDragEnd()
}

onMounted(async () => {
  // Only fetch if no data yet (prevents refetch on every mount)
  if (recentTicketsStore.recentTickets.length === 0) {
    await recentTicketsStore.fetchRecentTickets()
  }
})

</script>

<template>
  <div class="h-full flex flex-col">
    <!-- Loading (only on initial load) -->
    <div v-if="showLoading" class="p-1 space-y-0.5">
      <div v-for="i in 8" :key="i" class="h-7 bg-surface-hover rounded animate-pulse"></div>
    </div>

    <!-- List -->
    <div
      v-else-if="recentTicketsStore.recentTickets.length > 0"
      ref="listContainerRef"
      class="flex-1 min-h-0 overflow-y-auto"
      @drop="handleDrop"
      @dragover.prevent
    >
      <TransitionGroup name="ticket-list" tag="div" class="py-0.5 relative">
        <RouterLink
          v-for="(ticket, index) in recentTicketsStore.recentTickets"
          :key="ticket.id"
          :to="`/tickets/${ticket.id}`"
          class="ticket-item group flex items-center gap-1.5 px-2 py-1 mx-0.5 rounded hover:bg-surface-hover transition-colors cursor-grab select-none"
          :class="{
            'opacity-50': draggedIndex === index,
            'drop-above': draggedIndex !== null && dropTargetIndex === index && !isOutsideList,
            'drop-below': draggedIndex !== null && dropTargetIndex === index + 1 && index === recentTicketsStore.recentTickets.length - 1 && !isOutsideList
          }"
          draggable="true"
          @dragstart="handleDragStart(ticket, index, $event)"
          @drag="handleDrag"
          @dragend="handleLocalDragEnd"
          @dragover="handleDragOver(index, $event)"
          @dragleave="handleDragLeave"
          @contextmenu="handleTicketContextMenu(ticket, $event)"
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
            {{ formatCompactRelativeTime(ticket.last_viewed_at) }}
          </span>
        </RouterLink>
      </TransitionGroup>
    </div>

    <!-- Empty -->
    <div v-else class="flex-1 flex items-center justify-center p-2">
      <p class="text-xs text-tertiary">No recent tickets</p>
    </div>

    <!-- Context Menu. Always mounted; `:open` lets Popover's
         enter/leave fade-scale transition play. -->
    <ContextMenu
      :open="showContextMenu"
      :items="ticketContextMenuItems"
      :x="contextMenuPos.x"
      :y="contextMenuPos.y"
      @select="handleTicketContextMenuSelect"
      @close="showContextMenu = false"
    />

    <!-- Drag Preview - only show when dragging OUTSIDE the list -->
    <TicketDragPreview
      v-if="dragState.isDragging && dragState.source === 'recent-tickets' && dragState.ticket && dragState.position && isOutsideList"
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

/* Drop indicator using pseudo-elements */
.ticket-item {
  position: relative;
}

.ticket-item.drop-above::before,
.ticket-item.drop-below::after {
  content: '';
  position: absolute;
  left: 0.5rem;
  right: 0.5rem;
  height: 2px;
  background: var(--color-accent);
  border-radius: 1px;
  animation: dropIndicatorPulse 0.8s ease-in-out infinite;
}

.ticket-item.drop-above::before {
  top: -1px;
}

.ticket-item.drop-below::after {
  bottom: -1px;
}

@keyframes dropIndicatorPulse {
  0%, 100% { opacity: 0.7; }
  50% { opacity: 1; }
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
