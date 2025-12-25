<!-- KanbanBoard.vue -->
<script setup lang="ts">
import { ref, watch, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { projectService } from '@/services/projectService'
import UserAvatar from '@/components/UserAvatar.vue'
import AddTicketToProjectModal from './AddTicketToProjectModal.vue'
import StatusIndicator from '@/components/common/StatusIndicator.vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import { useKanbanDragDrop, type KanbanColumn } from '@/composables/useKanbanDragDrop'
import { formatCompactRelativeTime } from '@/utils/dateUtils'

const props = defineProps<{
  projectId: number;
}>()

const router = useRouter()
const isLoading = ref(true)
const error = ref<string | null>(null)

// Initialize columns with empty arrays
const columns = ref<KanbanColumn[]>([
  { id: 'open', title: 'Open', tickets: [] },
  { id: 'in-progress', title: 'In Progress', tickets: [] },
  { id: 'closed', title: 'Closed', tickets: [] }
])

const showAddTicketModal = ref(false)
const currentColumnId = ref<string | null>(null)
const projectTicketIds = ref<number[]>([])

// Fetch project tickets
async function fetchProjectTickets() {
  if (!props.projectId) return

  try {
    isLoading.value = true
    error.value = null

    const tickets = await projectService.getProjectTickets(props.projectId)
    projectTicketIds.value = tickets.map(ticket => ticket.id)

    // Reset and distribute tickets to columns
    columns.value.forEach(column => { column.tickets = [] })

    tickets.forEach(ticket => {
      const columnId = ticket.status === 'in-progress' ? 'in-progress'
        : ticket.status === 'closed' ? 'closed' : 'open'

      const column = columns.value.find(col => col.id === columnId)
      if (column) {
        column.tickets.push({
          id: ticket.id,
          title: ticket.title,
          assignee_uuid: ticket.assignee_user?.uuid || null,
          assignee_name: ticket.assignee_user?.name || ticket.assignee || null,
          assignee_avatar: ticket.assignee_user?.avatar_thumb || null,
          requester_uuid: ticket.requester_user?.uuid || null,
          requester_name: ticket.requester_user?.name || ticket.requester || null,
          requester_avatar: ticket.requester_user?.avatar_thumb || null,
          priority: ticket.priority as 'low' | 'medium' | 'high',
          status: ticket.status,
          modified: ticket.modified
        })
      }
    })
  } catch (err) {
    console.error('Failed to fetch project tickets:', err)
    error.value = 'Failed to load tickets. Please try again later.'
  } finally {
    isLoading.value = false
  }
}

// Handle external ticket drop (from recent tickets sidebar)
async function handleExternalTicketDrop(ticketId: number, targetColumnId: string) {
  // Check if ticket is already in project
  if (projectTicketIds.value.includes(ticketId)) {
    console.log(`Ticket ${ticketId} is already in this project`)
    return
  }

  try {
    // Add ticket to project
    await projectService.addTicketToProject(props.projectId, ticketId)
    console.log(`Added ticket ${ticketId} to project ${props.projectId}`)

    // Refresh to get the updated ticket list
    await fetchProjectTickets()
  } catch (err) {
    console.error('Failed to add ticket to project:', err)
    error.value = 'Failed to add ticket to project. Please try again.'
  }
}

// Use drag-drop composable (after fetchProjectTickets is defined)
const {
  dragState,
  handleDragStart,
  handleDragEnd,
  handleColumnDragOver,
  handleColumnDragLeave,
  handleColumnDrop,
  handleTouchStart,
  handleTouchMove,
  handleTouchEnd,
  handleTouchCancel,
  isDraggedTicket,
  isColumnDragOver
} = useKanbanDragDrop(columns, fetchProjectTickets, handleExternalTicketDrop)

watch(() => props.projectId, (newProjectId) => {
  if (newProjectId) fetchProjectTickets()
}, { immediate: true })

onMounted(() => {
  if (props.projectId) fetchProjectTickets()
})

const openTicket = (ticketId: number) => {
  if (!dragState.value.isDragging) {
    router.push(`/tickets/${ticketId}`)
  }
}

const createTicket = async (columnId: string) => {
  currentColumnId.value = columnId
  showAddTicketModal.value = true
}

const handleAddTicket = (ticketId: number) => {
  console.log(`Ticket ${ticketId} added to project ${props.projectId}`)
  fetchProjectTickets()
}
</script>

<template>
  <div>
    <!-- Error message -->
    <div v-if="error" class="bg-status-error-muted border border-status-error/30 text-status-error px-4 py-3 rounded-lg m-4">
      {{ error }}
    </div>

    <!-- Loading state -->
    <div v-if="isLoading" class="flex justify-center items-center py-8">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-accent"></div>
    </div>

    <!-- Kanban Board - horizontal scroll on smaller screens, fills width on large screens -->
    <div v-else class="flex gap-4 p-4 h-full xl:justify-center">
      <div
        v-for="column in columns"
        :key="column.id"
        class="w-72 flex-shrink-0 xl:flex-shrink xl:flex-grow xl:max-w-md flex flex-col bg-surface rounded-lg border border-default h-full min-h-[400px]"
        :class="{ 'ring-2 ring-accent/50': isColumnDragOver(column.id) }"
      >
          <!-- Column Header Background (for rounded corners) -->
          <div class="bg-surface-alt flex-shrink-0 px-4 pb-2 rounded-t-xl"></div>

          <!-- Column Header (sticky) -->
          <div class="bg-surface-alt flex-shrink-0 sticky top-0 z-10 transition-all duration-150 ease-out px-4 py-1">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                <StatusIndicator :status="column.id as 'open' | 'in-progress' | 'closed'" size="sm" />
                <h3 class="font-medium text-primary">{{ column.title }}</h3>
              </div>
              <span class="text-tertiary bg-surface-hover rounded-md text-xs px-2 py-1">
                {{ column.tickets.length }}
              </span>
            </div>
          </div>

          <!-- Header bottom spacing -->
          <div class="bg-surface-alt flex-shrink-0 px-4 pb-1"></div>

          <!-- Sticky border - sticks below the header -->
          <div class="sticky top-8 z-10 h-px shadow-sm bg-(--color-border-default)"></div>

          <!-- Column Content - full height drop zone -->
          <div
            class="flex-1 flex flex-col gap-2 p-3 relative overflow-y-auto"
            :data-column-id="column.id"
            @dragover="handleColumnDragOver(column.id, $event)"
            @dragleave="handleColumnDragLeave"
            @drop="handleColumnDrop(column.id, $event)"
          >
            <!-- Single global drop indicator for this column -->
            <div
              v-if="isColumnDragOver(column.id) && dragState.dropIndicatorY !== null"
              class="drop-indicator"
              :style="{ top: `${dragState.dropIndicatorY}px` }"
            />

            <!-- Tickets -->
            <div
              v-for="ticket in column.tickets"
              :key="ticket.id"
              :data-ticket-id="ticket.id"
              class="bg-surface rounded-lg border border-default p-3
                     cursor-grab hover:border-strong hover:shadow-sm transition-all group
                     select-none flex-shrink-0"
              :class="[
                { 'opacity-50 scale-95': isDraggedTicket(ticket.id) },
                { 'touch-none': dragState.isDragging }
              ]"
              draggable="true"
              @dragstart="handleDragStart(column.id, ticket, $event)"
              @dragend="handleDragEnd"
              @touchstart="handleTouchStart(column.id, ticket, $event)"
              @touchmove="handleTouchMove"
              @touchend="handleTouchEnd"
              @touchcancel="handleTouchCancel"
              @click="openTicket(ticket.id)"
            >
              <!-- Header: ID + Priority -->
              <div class="flex items-center justify-between gap-2 mb-2">
                <span class="text-xs text-tertiary font-mono">#{{ ticket.id }}</span>
                <PriorityIndicator :priority="ticket.priority" size="xs" />
              </div>

              <!-- Title -->
              <h4 class="text-sm font-medium text-primary line-clamp-2
                         group-hover:text-accent transition-colors leading-snug">
                {{ ticket.title }}
              </h4>

              <!-- Requester -->
              <div v-if="ticket.requester_name" class="flex items-center gap-1 mt-2 text-[11px] text-tertiary">
                <span>From:</span>
                <UserAvatar
                  :name="ticket.requester_name"
                  :avatar="ticket.requester_avatar"
                  size="xxs"
                  :showName="true"
                  :clickable="false"
                />
              </div>

              <!-- Footer: Assignee + Modified -->
              <div class="flex items-center justify-between mt-2.5 pt-2 border-t border-subtle">
                <div class="flex items-center gap-1 min-w-0">
                  <template v-if="ticket.assignee_name">
                    <UserAvatar
                      :name="ticket.assignee_name"
                      :avatar="ticket.assignee_avatar"
                      size="xxs"
                      :showName="false"
                      :clickable="false"
                    />
                    <span class="text-[11px] text-secondary truncate">{{ ticket.assignee_name }}</span>
                  </template>
                  <span v-else class="text-[11px] text-tertiary italic">Unassigned</span>
                </div>
                <span v-if="ticket.modified" class="text-[10px] text-tertiary flex-shrink-0">
                  {{ formatCompactRelativeTime(ticket.modified) }}
                </span>
              </div>
            </div>

            <!-- Empty state / Drop zone filler -->
            <div
              v-if="column.tickets.length === 0"
              class="flex-1 flex items-center justify-center text-tertiary text-sm
                     border-2 border-dashed border-subtle rounded-lg min-h-[200px]"
              :class="{ 'border-accent/50 bg-accent-muted': isColumnDragOver(column.id) }"
            >
              Drop tickets here
            </div>

            <!-- Spacer to push add button to bottom when there are tickets -->
            <div v-else class="flex-1 min-h-[40px]"></div>

            <!-- Add Ticket Button -->
            <button
              @click="createTicket(column.id)"
              class="w-full mt-auto p-2 text-sm text-tertiary
                     hover:text-accent transition-colors
                     flex items-center justify-center gap-1.5 flex-shrink-0"
            >
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
              </svg>
              Add ticket
            </button>
          </div>
        </div>
      </div>

    <!-- Floating drag preview for touch -->
    <div
      v-if="dragState.isDragging && dragState.touchDragPosition && dragState.draggedTicket"
      class="fixed pointer-events-none z-50 w-48"
      :style="{
        left: `${dragState.touchDragPosition.x}px`,
        top: `${dragState.touchDragPosition.y}px`,
        transform: 'translate(-50%, -50%)'
      }"
    >
      <div class="bg-surface-alt rounded-md border border-accent shadow-lg px-2.5 py-2">
        <div class="flex items-start gap-2">
          <div class="flex items-center gap-1.5 flex-shrink-0">
            <span class="text-[11px] text-tertiary font-mono">#{{ dragState.draggedTicket.ticket.id }}</span>
            <PriorityIndicator :priority="dragState.draggedTicket.ticket.priority" size="xs" />
          </div>
          <h4 class="text-xs font-medium text-primary line-clamp-2 flex-1">
            {{ dragState.draggedTicket.ticket.title }}
          </h4>
        </div>
      </div>
    </div>

    <!-- Add Ticket Modal -->
    <AddTicketToProjectModal
      :show="showAddTicketModal"
      :project-id="props.projectId"
      :existing-tickets="projectTicketIds"
      @close="showAddTicketModal = false"
      @add-ticket="handleAddTicket"
      @refresh="fetchProjectTickets"
    />
  </div>
</template>

<style scoped>
/* Line clamp utility for ticket titles */
.line-clamp-2 {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

/* Enhanced drag feedback */
[draggable="true"] {
  cursor: grab;
  -webkit-user-select: none;
  user-select: none;
}

[draggable="true"]:active {
  cursor: grabbing;
}

/* Single global drop indicator */
.drop-indicator {
  position: absolute;
  left: 16px;
  right: 16px;
  height: 2px;
  background-color: var(--color-accent);
  border-radius: 1px;
  pointer-events: none;
  z-index: 50;
  transform: translateY(-1px);
  transition: top 0.1s ease-out;
}

/* Prevent text selection and context menu during touch drag */
.touch-none {
  touch-action: none;
}

/* Floating drag preview animation */
@keyframes drag-pickup {
  from {
    transform: translate(-50%, -50%) scale(0.95);
    opacity: 0;
  }
  to {
    transform: translate(-50%, -50%) scale(1);
    opacity: 1;
  }
}

.fixed.pointer-events-none {
  animation: drag-pickup 0.15s ease-out forwards;
}

</style> 