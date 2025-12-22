<!-- KanbanBoard.vue -->
<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { projectService } from '@/services/projectService'
import ticketService from '@/services/ticketService'
import { STATUS_OPTIONS, type TicketStatus } from '@/constants/ticketOptions'
import UserAvatar from '@/components/UserAvatar.vue'
import AddTicketToProjectModal from './AddTicketToProjectModal.vue'
import StatusIndicator from '@/components/common/StatusIndicator.vue'

interface KanbanTicket {
  id: number;
  title: string;
  assignee: string;
  assignee_avatar?: string | null;
  priority: 'low' | 'medium' | 'high';
  status: string;
}

interface KanbanColumn {
  id: string;
  title: string;
  tickets: KanbanTicket[];
}

const props = defineProps<{
  projectId: number;
}>()

const router = useRouter()
const isLoading = ref(true)
const error = ref<string | null>(null)

// Initialize columns with empty arrays
const columns = ref<KanbanColumn[]>([
  {
    id: 'open',
    title: 'Open',
    tickets: []
  },
  {
    id: 'in-progress',
    title: 'In Progress',
    tickets: []
  },
  {
    id: 'closed',
    title: 'Closed',
    tickets: []
  }
])

// Enhanced drag state with intelligent column-based positioning
const dragState = ref<{
  draggedTicket: { columnId: string; ticketId: number; ticket: KanbanTicket } | null;
  dragOverColumn: string | null;
  insertIndex: number;
  isDragging: boolean;
}>({
  draggedTicket: null,
  dragOverColumn: null,
  insertIndex: -1,
  isDragging: false
})

const showAddTicketModal = ref(false)
const currentColumnId = ref<string | null>(null)
const projectTicketIds = ref<number[]>([])

// Fetch project tickets
const fetchProjectTickets = async () => {
  if (!props.projectId) return
  
  try {
    isLoading.value = true
    error.value = null
    
    // Fetch tickets for the project
    const tickets = await projectService.getProjectTickets(props.projectId)
    
    // Store all ticket IDs for the project to avoid duplicates
    projectTicketIds.value = tickets.map(ticket => ticket.id)
    
    // Reset all columns
    columns.value.forEach(column => {
      column.tickets = []
    })
    
    // Distribute tickets to appropriate columns based on status
    tickets.forEach(ticket => {
      const kanbanTicket: KanbanTicket = {
        id: ticket.id,
        title: ticket.title,
        assignee: ticket.assignee || 'Unassigned',
        assignee_avatar: ticket.assignee_avatar,
        priority: ticket.priority as 'low' | 'medium' | 'high',
        status: ticket.status
      }
      
      // Map ticket status to column
      let columnId: string
      switch (ticket.status) {
        case 'in-progress':
          columnId = 'in-progress'
          break
        case 'closed':
          columnId = 'closed'
          break
        case 'open':
        default:
          columnId = 'open'
          break
      }
      
      // Find the column and add the ticket
      const column = columns.value.find(col => col.id === columnId)
      if (column) {
        column.tickets.push(kanbanTicket)
      }
    })
  } catch (err) {
    console.error('Failed to fetch project tickets:', err)
    error.value = 'Failed to load tickets. Please try again later.'
  } finally {
    isLoading.value = false
  }
}

// Watch for changes to projectId
watch(() => props.projectId, (newProjectId) => {
  if (newProjectId) {
    fetchProjectTickets()
  }
}, { immediate: true })

// Fetch tickets on component mount
onMounted(() => {
  if (props.projectId) {
    fetchProjectTickets()
  }
})

// Enhanced drag start handler
const handleDragStart = (columnId: string, ticket: KanbanTicket, dragEvent: DragEvent) => {
  dragState.value.draggedTicket = { columnId, ticketId: ticket.id, ticket }
  dragState.value.isDragging = true
  
  if (dragEvent.dataTransfer) {
    dragEvent.dataTransfer.effectAllowed = 'move'
    dragEvent.dataTransfer.setData('text/plain', ticket.id.toString())
  }
}

// Enhanced drag end handler
const handleDragEnd = () => {
  dragState.value.draggedTicket = null
  dragState.value.dragOverColumn = null
  dragState.value.insertIndex = -1
  dragState.value.isDragging = false
}

// Intelligent column drag over handler
const handleColumnDragOver = (columnId: string, dragEvent: DragEvent) => {
  dragEvent.preventDefault()
  
  if (!dragState.value.draggedTicket) return
  
  dragState.value.dragOverColumn = columnId
  
  // Calculate insertion index based on cursor position
  const column = columns.value.find(col => col.id === columnId)
  if (!column || column.tickets.length === 0) {
    dragState.value.insertIndex = 0
    return
  }
  
  // Get the column container element
  const columnElement = dragEvent.currentTarget as HTMLElement
  const columnRect = columnElement.getBoundingClientRect()
  const cursorY = dragEvent.clientY
  
  // Find all ticket elements within this column
  const ticketElements = columnElement.querySelectorAll('[data-ticket-id]')
  let insertIndex = column.tickets.length // Default to end
  
  for (let i = 0; i < ticketElements.length; i++) {
    const ticketElement = ticketElements[i] as HTMLElement
    const ticketRect = ticketElement.getBoundingClientRect()
    const ticketCenter = ticketRect.top + ticketRect.height / 2
    
    if (cursorY < ticketCenter) {
      insertIndex = i
      break
    }
  }
  
  // Adjust for dragging within the same column
  if (dragState.value.draggedTicket.columnId === columnId) {
    const draggedTicketIndex = column.tickets.findIndex(t => t.id === dragState.value.draggedTicket?.ticketId)
    if (draggedTicketIndex !== -1 && draggedTicketIndex < insertIndex) {
      insertIndex--
    }
  }
  
  dragState.value.insertIndex = insertIndex
}

// Handle drop on columns
const handleColumnDrop = async (targetColumnId: string, dragEvent: DragEvent) => {
  dragEvent.preventDefault()
  
  if (!dragState.value.draggedTicket) return

  const sourceColumnId = dragState.value.draggedTicket.columnId
  const draggedTicketId = dragState.value.draggedTicket.ticketId
  const insertIndex = dragState.value.insertIndex
  
  const sourceColumn = columns.value.find(col => col.id === sourceColumnId)
  const targetColumn = columns.value.find(col => col.id === targetColumnId)
  
  if (!sourceColumn || !targetColumn) {
    handleDragEnd()
    return
  }

  // Remove ticket from source column
  const sourceTicketIndex = sourceColumn.tickets.findIndex(t => t.id === draggedTicketId)
  if (sourceTicketIndex === -1) {
    handleDragEnd()
    return
  }

  const [ticket] = sourceColumn.tickets.splice(sourceTicketIndex, 1)
  
  // Insert at calculated position
  const finalInsertIndex = Math.max(0, Math.min(insertIndex, targetColumn.tickets.length))
  targetColumn.tickets.splice(finalInsertIndex, 0, ticket)

  // Update ticket status if moving to a different column
  if (sourceColumnId !== targetColumnId) {
    let newStatus: TicketStatus
    switch (targetColumnId) {
      case 'in-progress':
        newStatus = 'in-progress'
        break
      case 'closed':
        newStatus = 'closed'
        break
      case 'open':
      default:
        newStatus = 'open'
        break
    }
    
    try {
      await ticketService.updateTicket(ticket.id, { 
        status: newStatus,
        modified: new Date().toISOString()
      })
      console.log(`Updated ticket ${ticket.id} status to ${newStatus}`)
    } catch (err) {
      console.error(`Failed to update ticket status:`, err)
      await fetchProjectTickets()
    }
  }
  
  handleDragEnd()
}

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

const getPriorityColor = (priority: string) => {
  switch (priority) {
    case 'high':
      return 'bg-priority-high-muted text-priority-high'
    case 'medium':
      return 'bg-priority-medium-muted text-priority-medium'
    case 'low':
      return 'bg-priority-low-muted text-priority-low'
    default:
      return 'bg-surface-alt text-secondary'
  }
}

// Helper to get visual feedback for insertion position
const getInsertionLinePosition = (columnId: string, insertIndex: number): 'top' | 'bottom' | { after: number } | null => {
  if (dragState.value.dragOverColumn !== columnId || !dragState.value.isDragging) {
    return null
  }
  
  const column = columns.value.find(col => col.id === columnId)
  if (!column) return null
  
  if (insertIndex === 0) {
    return 'top'
  } else if (insertIndex >= column.tickets.length) {
    return 'bottom'
  } else {
    return { after: insertIndex - 1 }
  }
}

// Helper to check if insertion indicator should show after a specific ticket
const shouldShowInsertionAfter = (columnId: string, ticketIndex: number): boolean => {
  if (dragState.value.dragOverColumn !== columnId || !dragState.value.isDragging) {
    return false
  }
  
  const column = columns.value.find(col => col.id === columnId)
  if (!column) return false
  
  return dragState.value.insertIndex === ticketIndex + 1
}
</script>

<template>
  <div class="h-full flex flex-col relative">
    <!-- Error message -->
    <div v-if="error" class="bg-status-error-muted border border-status-error/30 text-status-error px-4 py-3 rounded-lg mb-4">
      {{ error }}
    </div>

    <!-- Loading state -->
    <div v-if="isLoading" class="flex justify-center items-center py-8">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-accent"></div>
    </div>
    
    <div v-else class="flex-1 overflow-x-auto">
      <div class="flex gap-6 p-6 min-w-max h-full">
        <div
          v-for="column in columns"
          :key="column.id"
          class="w-80 flex flex-col bg-surface-alt rounded-xl border border-default h-full overflow-hidden"
          :class="{
            'ring-2 ring-accent/50': dragState.dragOverColumn === column.id && dragState.isDragging
          }"
        >
          <!-- Column Header -->
          <div class="px-4 py-3 bg-surface border-b border-default sticky top-0" style="z-index: 1;">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                <StatusIndicator :status="column.id as 'open' | 'in-progress' | 'closed'" size="sm" />
                <h3 class="font-medium text-primary">{{ column.title }}</h3>
              </div>
              <span class="text-xs text-tertiary bg-surface-hover px-2 py-1 rounded-md">{{ column.tickets.length }}</span>
            </div>
          </div>

          <!-- Column Content -->
          <div
            class="flex-1 flex flex-col gap-3 p-4 overflow-y-auto hide-vertical-scrollbar relative"
            @dragover="handleColumnDragOver(column.id, $event)"
            @drop="handleColumnDrop(column.id, $event)"
          >
            <!-- Insertion indicator at top -->
            <div
              v-if="getInsertionLinePosition(column.id, dragState.insertIndex) === 'top'"
              class="absolute top-4 left-4 right-4 h-0.5 bg-accent rounded-full z-20 transition-all duration-200"
            ></div>
            
            <!-- Tickets -->
            <div
              v-for="(ticket, index) in column.tickets"
              :key="ticket.id"
              :data-ticket-id="ticket.id"
              class="relative bg-surface rounded-lg border border-subtle p-3 cursor-move hover:border-default hover:bg-surface-hover transition-all duration-200 group"
              :class="{
                'opacity-50 scale-95': dragState.draggedTicket?.ticketId === ticket.id,
              }"
              draggable="true"
              @dragstart="handleDragStart(column.id, ticket, $event)"
              @dragend="handleDragEnd"
              @click="openTicket(ticket.id)"
            >
              <!-- Insertion indicator after this ticket -->
              <div
                v-if="shouldShowInsertionAfter(column.id, index)"
                class="absolute -bottom-1.5 left-0 right-0 h-0.5 bg-accent rounded-full z-20 transition-all duration-200"
              ></div>
              
              <div class="flex flex-col gap-3">
                <!-- Ticket Title -->
                <h4 class="text-sm font-medium text-primary group-hover:text-accent transition-colors line-clamp-2">
                  {{ ticket.title }}
                </h4>
                
                <!-- Ticket Details -->
                <div class="flex items-center justify-between">
                  <!-- Assignee -->
                  <div class="flex items-center gap-2 min-w-0 flex-1">
                    <UserAvatar 
                      v-if="ticket.assignee && ticket.assignee !== 'Unassigned'" 
                      :name="ticket.assignee" 
                      :avatar="ticket.assignee_avatar"
                      size="xs" 
                      :showName="true"
                      :clickable="false"
                      class="text-xs"
                    />
                    <span v-else class="text-xs text-tertiary">Unassigned</span>
                  </div>
                  
                  <!-- Priority Badge -->
                  <div
                    class="px-2 py-1 rounded-md text-xs font-medium border flex-shrink-0"
                    :class="{
                      'bg-priority-high-muted text-priority-high border-priority-high/30': ticket.priority === 'high',
                      'bg-priority-medium-muted text-priority-medium border-priority-medium/30': ticket.priority === 'medium',
                      'bg-priority-low-muted text-priority-low border-priority-low/30': ticket.priority === 'low'
                    }"
                  >
                    {{ ticket.priority }}
                  </div>
                </div>
              </div>
            </div>

            <!-- Insertion indicator at bottom -->
            <div
              v-if="getInsertionLinePosition(column.id, dragState.insertIndex) === 'bottom'"
              class="h-0.5 bg-accent rounded-full transition-all duration-200"
            ></div>

            <!-- Empty state indicator -->
            <div
              v-if="column.tickets.length === 0"
              class="flex-1 flex items-center justify-center text-tertiary text-sm border-2 border-dashed border-subtle rounded-lg py-8"
              :class="{
                'border-accent/50 bg-accent-muted': dragState.dragOverColumn === column.id && dragState.isDragging
              }"
            >
              Drop tickets here
            </div>

            <!-- Add Ticket Button -->
            <button
              @click="createTicket(column.id)"
              class="w-full mt-4 p-3 bg-surface border border-subtle rounded-lg text-sm text-tertiary hover:text-accent hover:bg-accent-muted hover:border-accent/30 transition-all duration-200 flex items-center justify-center gap-2"
            >
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
              </svg>
              Add ticket
            </button>
          </div>
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
/* Only hide vertical scrollbars for ticket columns */
.hide-vertical-scrollbar::-webkit-scrollbar {
  display: none;
}

.hide-vertical-scrollbar {
  -ms-overflow-style: none;  /* IE and Edge */
  scrollbar-width: none;  /* Firefox */
}

/* Style horizontal scrollbar */
.overflow-x-auto::-webkit-scrollbar {
  height: 12px;
  display: block;
}

.overflow-x-auto::-webkit-scrollbar-track {
  background: var(--bg-surface-alt);
  border-radius: 6px;
}

.overflow-x-auto::-webkit-scrollbar-thumb {
  background: var(--bg-surface);
  border-radius: 6px;
}

.overflow-x-auto::-webkit-scrollbar-thumb:hover {
  background: var(--bg-surface-hover);
}

/* Ensure kanban content stays below main header */
.relative {
  z-index: 0;
}

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
}

[draggable="true"]:active {
  cursor: grabbing;
}

/* Smooth transitions for drag states */
.transition-all {
  transition: all 0.2s ease-in-out;
}
</style> 