<script setup lang="ts">
import { useRouter } from 'vue-router'
import { shareableRouteUrl } from '@/utils/shareUrl'
import { useRecentTicketsStore } from '@/stores/recentTickets'
import { useWorkflowStatesStore } from '@nosdesk/core/stores/workflowStates'
import { ref, onMounted, computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { formatCompactRelativeTime } from '@nosdesk/core/utils/dateUtils'
import StatusIndicator from '@/components/common/StatusIndicator.vue'
import TicketDragPreview from '@/components/common/TicketDragPreview.vue'
import ContextMenu from '@/components/common/ContextMenu.vue'
import type { MenuItem } from '@/components/common/ContextMenu.vue'
import { useTicketDrag, type DraggableTicket, shouldSuppressTicketDrop } from '@/composables/useTicketDrag'
import { useClipboard } from '@/composables/useClipboard'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import type { RecentTicket } from '@nosdesk/core/types/ticket'
import type { WorkflowStateCategory } from '@nosdesk/core/types/workflow'

const router = useRouter()
const recentTicketsStore = useRecentTicketsStore()
const ticketsStore = useSyncTicketsStore()
const wf = useWorkflowStatesStore()
const fluent = useFluent()
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

// Recent tickets currently sitting in a terminal state (done or
// cancelled). Drives the "clear done & cancelled" bulk action.
const terminalRecentIds = computed(() =>
  recentTicketsStore.recentTickets
    .filter((t) => {
      const category = wf.findById(t.workflow_state_id ?? -1)?.category
      return category === 'done' || category === 'cancelled'
    })
    .map((t) => t.id),
)

// Context menu items. Labels resolve through Fluent so the menu
// reads in the active locale. Icon paths stay literal SVG d-attrs;
// `id` values are stable action keys.
const ticketContextMenuItems = computed<MenuItem[]>(() => {
  const items: MenuItem[] = [
    { id: 'open-new-tab', label: fluent.$t('recent-tickets-context-open-new-tab'), icon: 'M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25' },
    { id: 'copy-link', label: fluent.$t('recent-tickets-context-copy-link'), icon: 'M13.19 8.688a4.5 4.5 0 011.242 7.244l-4.5 4.5a4.5 4.5 0 01-6.364-6.364l1.757-1.757m13.35-.622l1.757-1.757a4.5 4.5 0 00-6.364-6.364l-4.5 4.5a4.5 4.5 0 001.242 7.244' },
    { id: 'remove-recent', label: fluent.$t('recent-tickets-context-remove'), icon: 'M6 18L18 6M6 6l12 12', danger: true, divider: true },
  ]
  // Only offer the bulk clear when there's something terminal to clear.
  if (terminalRecentIds.value.length > 0) {
    items.push({ id: 'clear-terminal', label: fluent.$t('recent-tickets-context-clear-done', { count: terminalRecentIds.value.length }), icon: 'M6 7.5l.75 11.25a1.5 1.5 0 001.5 1.4h7.5a1.5 1.5 0 001.5-1.4L19.5 7.5M9.75 7.5V5.25A1.5 1.5 0 0111.25 3.75h1.5a1.5 1.5 0 011.5 1.5V7.5M4.5 7.5h15', danger: true, divider: true })
  }
  return items
})

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

  // Workspace-scoped in path mode so the link/new-tab opens the right tenant.
  const ticketUrl = shareableRouteUrl('ticket-view', { id: String(ticket.id) })

  switch (actionId) {
    case 'open-new-tab':
      window.open(ticketUrl, '_blank')
      break

    case 'copy-link':
      await copy(ticketUrl)
      break

    case 'remove-recent':
      await recentTicketsStore.removeTicket(ticket.id)
      break

    case 'clear-terminal':
      await recentTicketsStore.removeManyTickets(terminalRecentIds.value)
      break
  }
}

// Local drag state for reordering
const draggedIndex = ref<number | null>(null)
const dropTargetIndex = ref<number | null>(null)
const isOutsideList = ref(false)
const listContainerRef = ref<HTMLElement | null>(null)
/** Suppress the click that browsers fire after a drag ends. */
const suppressNextClick = ref(false)

// Convert store ticket to draggable ticket format
const toDraggableTicket = (ticket: RecentTicket): DraggableTicket => {
  const pooled = ticketsStore.byId(ticket.id).value
  const category = (wf.findById(ticket.workflow_state_id ?? -1)?.category ?? 'backlog') as WorkflowStateCategory
  const priority = pooled?.priority
  return {
    id: ticket.id,
    title: ticket.title,
    category,
    assigneeUuid: ticket.assignee ?? pooled?.assignee_uuid ?? null,
    priority: priority === 'urgent' ? 'high' : priority,
  }
}

const isDragging = computed(() =>
  dragState.value.isDragging && dragState.value.source === 'recent-tickets',
)

function openTicket(ticketId: number, event: MouseEvent): void {
  if (suppressNextClick.value) {
    event.preventDefault()
    suppressNextClick.value = false
    return
  }
  router.push(`/tickets/${ticketId}`)
}

// Custom drag start - track the dragged index
const handleDragStart = (ticket: RecentTicket, index: number, event: DragEvent) => {
  suppressNextClick.value = false
  draggedIndex.value = index
  isOutsideList.value = false
  document.body.classList.add('cursor-grabbing')
  baseDragStart(toDraggableTicket(ticket), 'recent-tickets', event)
}

// Custom drag handler - check if we're inside or outside the list
const handleDrag = (event: DragEvent) => {
  if (event.clientX !== 0 || event.clientY !== 0) {
    suppressNextClick.value = true
  }
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
  event.stopPropagation()

  if (shouldSuppressTicketDrop()) {
    resetDragState()
    return
  }

  if (draggedIndex.value !== null && dropTargetIndex.value !== null && !isOutsideList.value) {
    let toIndex = dropTargetIndex.value
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

// `isLoading` from the store is the first-fetch-only signal (no cached
// or persisted data yet). With localStorage hydration this is rarely
// true for a returning user; we use it only to stay quiet during a
// genuine cold load instead of flashing the "empty" message.
const showLoading = computed(() => recentTicketsStore.isLoading)

onMounted(() => {
  // Always refresh in the background. When the store hydrated from
  // localStorage the cached rows render instantly and this refetch
  // updates them silently (isLoading stays false because data is
  // already defined), so there's no skeleton and no height jump.
  recentTicketsStore.fetchRecentTickets()
})

</script>

<template>
  <div class="h-full flex flex-col">
    <!-- List (cache-first: hydrated from localStorage, renders instantly) -->
    <div
      v-if="recentTicketsStore.recentTickets.length > 0"
      ref="listContainerRef"
      class="flex-1 min-h-0 overflow-y-auto"
      @drop="handleDrop"
      @dragover.prevent
    >
      <TransitionGroup name="ticket-list" tag="div" class="py-0.5 relative">
        <div
          v-for="(ticket, index) in recentTicketsStore.recentTickets"
          :key="ticket.id"
          role="link"
          tabindex="0"
          class="ticket-item group flex items-center gap-1.5 px-2 py-1 mx-0.5 rounded hover:bg-surface-hover transition-[colors,opacity,transform,box-shadow] cursor-grab active:cursor-grabbing select-none"
          :class="{
            'ticket-item--source': draggedIndex === index,
            'drop-above': draggedIndex !== null && dropTargetIndex === index && !isOutsideList,
            'drop-below': draggedIndex !== null && dropTargetIndex === index + 1 && !isOutsideList
          }"
          draggable="true"
          @click="openTicket(ticket.id, $event)"
          @keydown.enter="openTicket(ticket.id, $event as unknown as MouseEvent)"
          @dragstart="handleDragStart(ticket, index, $event)"
          @drag="handleDrag"
          @dragend="handleLocalDragEnd"
          @dragover="handleDragOver(index, $event)"
          @dragleave="handleDragLeave"
          @drop="handleDrop"
          @contextmenu="handleTicketContextMenu(ticket, $event)"
          @touchstart="handleTouchStart(toDraggableTicket(ticket), 'recent-tickets', $event)"
          @touchmove="handleTouchMove"
          @touchend="handleTouchEnd"
          @touchcancel="handleTouchCancel"
        >
          <!-- Status indicator -->
          <StatusIndicator :category="wf.findById(ticket.workflow_state_id ?? -1)?.category ?? 'backlog'" size="xs" />

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
        </div>
      </TransitionGroup>
    </div>

    <!-- Cold load with no cached data: stay quiet and let the data fill
         in, rather than flashing the "empty" message before it lands. -->
    <div v-else-if="showLoading" class="flex-1"></div>

    <!-- Empty -->
    <div v-else class="flex-1 flex items-center justify-center p-2">
      <p class="text-xs text-tertiary">{{ $t('recent-tickets-empty') }}</p>
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

    <!-- Custom preview follows the cursor for the whole drag. -->
    <TicketDragPreview
      v-if="isDragging && dragState.ticket && dragState.position"
      :ticket="dragState.ticket"
      :position="dragState.position"
    />
  </div>
</template>

<style scoped>
/* Drop indicator using pseudo-elements */
.ticket-item {
  position: relative;
}

.ticket-item--source {
  opacity: 0.35;
  transform: scale(0.98);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-accent) 25%, transparent);
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
