<!-- GanttPlanner.vue -->
<script setup lang="ts">
import { formatDate as formatDateUtil, formatDateTime } from '@/utils/dateUtils';
import { ref, onMounted, computed, watch } from 'vue'
import { useRouter } from 'vue-router'
import { projectService } from '@/services/projectService'
import ticketService from '@/services/ticketService'
import UserAvatar from '@/components/UserAvatar.vue'
import StatusBadge from '@/components/StatusBadge.vue'

const props = defineProps<{
  projectId: number;
  tickets: any[]; // Using any[] until we have a proper type
}>()

const router = useRouter()
const isLoading = ref(false)
const error = ref<string | null>(null)
const ganttTickets = ref<any[]>([])
const timeScale = ref<'day' | 'week' | 'month'>('week')
const today = new Date()
const startDate = ref(new Date(today.getFullYear(), today.getMonth(), 1)) // First day of current month
const endDate = ref(new Date(today.getFullYear(), today.getMonth() + 3, 0)) // Last day of +3 months

// Drag state
const isDragging = ref(false)
const draggedTask = ref<any>(null)
const dragStartX = ref(0)
const dragStartLeft = ref(0)
const dragStartWidth = ref(0)
const dragMode = ref<'move' | 'resize-start' | 'resize-end'>('move')
const wasDragging = ref(false)

// Tooltip state
const hoveredTask = ref<any>(null)
const tooltipPosition = ref({ x: 0, y: 0 })

// Convert tickets to Gantt format
const processTickets = () => {
  if (!props.tickets || props.tickets.length === 0) {
    ganttTickets.value = []
    return
  }

  ganttTickets.value = props.tickets.map(ticket => {
    // Helper to parse date from various field names
    const getDate = (fieldNames: string[], defaultDate: Date | null): Date | null => {
      for (const field of fieldNames) {
        if (ticket[field]) {
          const date = new Date(ticket[field])
          if (!isNaN(date.getTime())) {
            return date
          }
        }
      }
      return defaultDate
    }
    
    // Get created date from various possible fields
    const createdFields = ['created_at', 'createdAt', 'created', 'dateCreated', 'createDate']
    const start = getDate(createdFields, new Date()) || new Date()
    
    // Get due date from various possible fields
    const dueFields = ['due_date', 'dueDate', 'due', 'deadline', 'target_date', 'targetDate', 'end_date', 'endDate']
    let end = getDate(dueFields, null)
    
    // If no due date, calculate based on priority
    if (!end) {
      end = new Date(start)
      switch (ticket.priority) {
        case 'high':
          end.setDate(end.getDate() + 3) // 3 days for high priority
          break
        case 'medium':
          end.setDate(end.getDate() + 7) // 7 days for medium priority
          break
        case 'low':
          end.setDate(end.getDate() + 14) // 14 days for low priority
          break
        default:
          end.setDate(end.getDate() + 7) // Default 7 days
      }
    }
    
    // Ensure end is after start
    if (!end || end <= start) {
      end = new Date(start)
      end.setDate(end.getDate() + 1)
    }

    // Define color based on priority
    let color
    switch (ticket.priority) {
      case 'high':
        color = '#ef4444' // Red
        break
      case 'medium':
        color = '#f59e0b' // Amber
        break
      case 'low':
        color = '#3b82f6' // Blue
        break
      default:
        color = '#6b7280' // Gray
    }

    // Calculate progress based on status
    let progress = 0
    switch (ticket.status) {
      case 'closed':
      case 'completed':
      case 'done':
        progress = 100
        break
      case 'in-progress':
      case 'in_progress':
      case 'active':
      case 'working':
        progress = 50
        break
      case 'open':
      case 'new':
      case 'todo':
      case 'pending':
        progress = 0
        break
      default:
        progress = 0
    }

    return {
      id: ticket.id,
      title: ticket.title || `Ticket #${ticket.id}`,
      start,
      end,
      color,
      progress,
      status: ticket.status,
      assignee: ticket.assignee_name || ticket.assignee || ticket.assigned_to || ticket.assignedTo,
      priority: ticket.priority,
      description: ticket.description
    }
  })
  
  // Sort tickets by start date
  ganttTickets.value.sort((a, b) => a.start.getTime() - b.start.getTime())
  
  // Adjust date range to fit all tickets
  if (ganttTickets.value.length > 0) {
    const minDate = new Date(Math.min(...ganttTickets.value.map(t => t.start.getTime())))
    const maxDate = new Date(Math.max(...ganttTickets.value.map(t => t.end.getTime())))
    
    // Add some padding
    minDate.setDate(minDate.getDate() - 7)
    maxDate.setDate(maxDate.getDate() + 7)
    
    startDate.value = minDate
    endDate.value = maxDate
  }
}

// Initialize data when tickets prop changes
watch(() => props.tickets, () => {
  processTickets()
}, { immediate: true })

// Date formatting helpers
const formatDate = (date: Date) => {
  return formatDateUtil(date, 'MMM d')
}

const formatHeaderDate = (date: Date) => {
  const month = formatDateUtil(date, 'MMM')
  const day = date.getDate()
  return `${month} ${day}`
}

// Generate time units for the header based on timeScale
const timeUnits = computed(() => {
  const units = []
  let current = new Date(startDate.value)
  
  while (current <= endDate.value) {
    units.push(new Date(current))
    
    // Increment based on timeScale
    if (timeScale.value === 'day') {
      current.setDate(current.getDate() + 1)
    } else if (timeScale.value === 'week') {
      current.setDate(current.getDate() + 7)
    } else {
      current.setMonth(current.getMonth() + 1)
    }
  }
  
  return units
})

// Helper to calculate position and width of a task bar
const getTaskBarStyle = (task: any) => {
  // Calculate which time unit the task starts in and how many units it spans
  const millisecondsPerUnit = timeScale.value === 'day' ? 
    (24 * 60 * 60 * 1000) : 
    timeScale.value === 'week' ? 
    (7 * 24 * 60 * 60 * 1000) : 
    (30 * 24 * 60 * 60 * 1000) // approximate month
  
  // Find which grid unit the task starts in
  let startUnitIndex = 0
  for (let i = 0; i < timeUnits.value.length; i++) {
    const unitStart = timeUnits.value[i]
    const unitEnd = new Date(unitStart.getTime() + millisecondsPerUnit)
    
    if (task.start >= unitStart && task.start < unitEnd) {
      startUnitIndex = i
      break
    } else if (task.start < unitStart) {
      startUnitIndex = Math.max(0, i - 1)
      break
    }
  }
  
  // Find which grid unit the task ends in
  let endUnitIndex = timeUnits.value.length - 1
  for (let i = 0; i < timeUnits.value.length; i++) {
    const unitStart = timeUnits.value[i]
    const unitEnd = new Date(unitStart.getTime() + millisecondsPerUnit)
    
    if (task.end > unitStart && task.end <= unitEnd) {
      endUnitIndex = i
      break
    } else if (task.end <= unitStart) {
      endUnitIndex = Math.max(0, i - 1)
      break
    }
  }
  
  // Calculate position and width based on grid units
  const totalUnits = timeUnits.value.length
  const startPercent = (startUnitIndex / totalUnits) * 100
  const spanUnits = Math.max(1, endUnitIndex - startUnitIndex + 1)
  const widthPercent = (spanUnits / totalUnits) * 100
  
  // Fine-tune position within the start unit
  const startUnit = timeUnits.value[startUnitIndex]
  const startUnitEnd = new Date(startUnit.getTime() + millisecondsPerUnit)
  const offsetWithinUnit = Math.max(0, (task.start.getTime() - startUnit.getTime()) / millisecondsPerUnit)
  const adjustedStartPercent = startPercent + (offsetWithinUnit * (100 / totalUnits))
  
  // Fine-tune width based on actual task duration within units
  const endUnit = timeUnits.value[endUnitIndex]
  const endUnitEnd = new Date(endUnit.getTime() + millisecondsPerUnit)
  const endOffsetWithinUnit = Math.min(1, (task.end.getTime() - endUnit.getTime()) / millisecondsPerUnit)
  const adjustedWidthPercent = widthPercent + (endOffsetWithinUnit * (100 / totalUnits)) - (offsetWithinUnit * (100 / totalUnits))
  
  return {
    left: `${adjustedStartPercent}%`,
    width: `${Math.max(1, adjustedWidthPercent)}%`,
    backgroundColor: task.color,
    opacity: task.status === 'closed' ? 0.7 : 1
  }
}

// Navigate to ticket detail
const openTicket = (ticketId: number) => {
  // Don't navigate if we just finished dragging
  if (wasDragging.value) {
    wasDragging.value = false // Reset the flag
    return
  }
  router.push(`/tickets/${ticketId}`)
}

// Change time scale
const setTimeScale = (scale: 'day' | 'week' | 'month') => {
  timeScale.value = scale
}

// Adjust date range
const zoomIn = () => {
  const currentRange = endDate.value.getTime() - startDate.value.getTime()
  const newRange = currentRange * 0.7
  
  // Recalculate end date based on the new range
  const newEndDate = new Date(startDate.value.getTime() + newRange)
  endDate.value = newEndDate
}

const zoomOut = () => {
  const currentRange = endDate.value.getTime() - startDate.value.getTime()
  const newRange = currentRange * 1.3
  
  // Recalculate end date based on the new range
  const newEndDate = new Date(startDate.value.getTime() + newRange)
  endDate.value = newEndDate
}

const moveRangeLeft = () => {
  const rangeSize = endDate.value.getTime() - startDate.value.getTime()
  const moveAmount = rangeSize * 0.25 // Move 25% of the range
  
  startDate.value = new Date(startDate.value.getTime() - moveAmount)
  endDate.value = new Date(endDate.value.getTime() - moveAmount)
}

const moveRangeRight = () => {
  const rangeSize = endDate.value.getTime() - startDate.value.getTime()
  const moveAmount = rangeSize * 0.25 // Move 25% of the range
  
  startDate.value = new Date(startDate.value.getTime() + moveAmount)
  endDate.value = new Date(endDate.value.getTime() + moveAmount)
}

// Reset to today
const resetView = () => {
  startDate.value = new Date(today.getFullYear(), today.getMonth(), 1)
  endDate.value = new Date(today.getFullYear(), today.getMonth() + 3, 0)
}

// Helper function to check if a date is the same day as today
const isSameDay = (date: Date, today: Date) => {
  return date.toDateString() === today.toDateString()
}

// Drag and drop handlers
const startDrag = (event: MouseEvent, task: any, mode: 'move' | 'resize-start' | 'resize-end' = 'move') => {
  isDragging.value = true
  draggedTask.value = task
  dragMode.value = mode
  dragStartX.value = event.clientX
  wasDragging.value = false // Reset at start of drag
  
  const taskElement = event.currentTarget as HTMLElement
  const rect = taskElement.getBoundingClientRect()
  dragStartLeft.value = rect.left
  dragStartWidth.value = rect.width
  
  // Add event listeners for drag
  document.addEventListener('mousemove', handleDrag)
  document.addEventListener('mouseup', endDrag)
  
  // Prevent text selection during drag
  event.preventDefault()
}

const handleDrag = (event: MouseEvent) => {
  if (!isDragging.value || !draggedTask.value) return
  
  const deltaX = event.clientX - dragStartX.value
  
  // If moved more than 3 pixels, consider it a drag operation
  if (Math.abs(deltaX) > 3) {
    wasDragging.value = true
  }
  
  const chartTimeline = document.querySelector('.gantt-row-timeline')
  if (!chartTimeline) return
  
  const chartWidth = chartTimeline.clientWidth
  const totalUnits = timeUnits.value.length
  const pixelsPerUnit = chartWidth / totalUnits
  const unitsDelta = Math.round(deltaX / pixelsPerUnit)
  
  // Calculate milliseconds per unit based on time scale
  const millisecondsPerUnit = timeScale.value === 'day' ? 
    (24 * 60 * 60 * 1000) : 
    timeScale.value === 'week' ? 
    (7 * 24 * 60 * 60 * 1000) : 
    (30 * 24 * 60 * 60 * 1000) // approximate month
  
  const timeDelta = unitsDelta * millisecondsPerUnit
  
  if (dragMode.value === 'move') {
    // Update both start and end dates
    const newStart = new Date(draggedTask.value.start.getTime() + timeDelta)
    const newEnd = new Date(draggedTask.value.end.getTime() + timeDelta)
    
    // Update the task in the array (this will trigger reactivity)
    const taskIndex = ganttTickets.value.findIndex(t => t.id === draggedTask.value.id)
    if (taskIndex !== -1) {
      ganttTickets.value[taskIndex] = {
        ...ganttTickets.value[taskIndex],
        start: newStart,
        end: newEnd
      }
    }
  } else if (dragMode.value === 'resize-start') {
    // Update start date only
    const newStart = new Date(draggedTask.value.start.getTime() + timeDelta)
    
    // Don't let start go past end
    if (newStart < draggedTask.value.end) {
      const taskIndex = ganttTickets.value.findIndex(t => t.id === draggedTask.value.id)
      if (taskIndex !== -1) {
        ganttTickets.value[taskIndex] = {
          ...ganttTickets.value[taskIndex],
          start: newStart
        }
      }
    }
  } else if (dragMode.value === 'resize-end') {
    // Update end date only
    const newEnd = new Date(draggedTask.value.end.getTime() + timeDelta)
    
    // Don't let end go before start
    if (newEnd > draggedTask.value.start) {
      const taskIndex = ganttTickets.value.findIndex(t => t.id === draggedTask.value.id)
      if (taskIndex !== -1) {
        ganttTickets.value[taskIndex] = {
          ...ganttTickets.value[taskIndex],
          end: newEnd
        }
      }
    }
  }
}

const endDrag = async () => {
  if (!isDragging.value || !draggedTask.value) return
  
  // Get the updated task
  const updatedTask = ganttTickets.value.find(t => t.id === draggedTask.value.id)
  if (updatedTask) {
    // Update the ticket in the backend
    try {
      await ticketService.updateTicket(updatedTask.id, {
        due_date: updatedTask.end.toISOString().split('T')[0], // Format as YYYY-MM-DD
        // You might also want to update a start_date field if your backend supports it
      } as any)
    } catch (error) {
      console.error('Failed to update ticket dates:', error)
      // Revert the change
      processTickets()
    }
  }
  
  // Clean up
  isDragging.value = false
  draggedTask.value = null
  document.removeEventListener('mousemove', handleDrag)
  document.removeEventListener('mouseup', endDrag)
}

// Tooltip handlers
const showTooltip = (event: MouseEvent, task: any) => {
  hoveredTask.value = task
  tooltipPosition.value = {
    x: event.clientX,
    y: event.clientY
  }
}

const hideTooltip = () => {
  hoveredTask.value = null
}
</script>

<template>
  <div class="h-full flex flex-col relative">
    <!-- Error message -->
    <div v-if="error" class="bg-red-500/20 border border-red-500/50 text-red-200 px-4 py-3 rounded-lg mb-4">
      {{ error }}
    </div>
    
    <!-- Loading state -->
    <div v-if="isLoading" class="flex justify-center items-center py-8">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500"></div>
    </div>
    
    <div v-else class="flex-1 flex flex-col gap-2">
      <!-- Controls -->
      <div class="flex items-center justify-between mb-2 bg-surface-alt p-2 rounded-lg">
        <div class="flex items-center gap-2">
          <button 
            @click="setTimeScale('day')" 
            class="px-2 py-0.5 text-xs rounded-md transition-colors"
            :class="timeScale === 'day' ? 'bg-blue-600 text-white' : 'bg-surface text-secondary hover:bg-surface-hover'"
          >
            Day
          </button>
          <button 
            @click="setTimeScale('week')" 
            class="px-2 py-0.5 text-xs rounded-md transition-colors"
            :class="timeScale === 'week' ? 'bg-blue-600 text-white' : 'bg-surface text-secondary hover:bg-surface-hover'"
          >
            Week
          </button>
          <button 
            @click="setTimeScale('month')" 
            class="px-2 py-0.5 text-xs rounded-md transition-colors"
            :class="timeScale === 'month' ? 'bg-blue-600 text-white' : 'bg-surface text-secondary hover:bg-surface-hover'"
          >
            Month
          </button>
        </div>
        
        <div class="flex items-center gap-2">
          <button
            @click="moveRangeLeft"
            class="p-1 bg-surface text-secondary rounded-md hover:bg-surface-hover transition-colors"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" />
            </svg>
          </button>
          
          <button
            @click="resetView"
            class="px-1.5 py-0.5 bg-surface text-secondary rounded-md hover:bg-surface-hover transition-colors text-xs"
          >
            Today
          </button>
          
          <button
            @click="moveRangeRight"
            class="p-1 bg-surface text-secondary rounded-md hover:bg-surface-hover transition-colors"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
            </svg>
          </button>
          
          <div class="h-4 border-r border-default mx-1"></div>
          
          <button
            @click="zoomIn"
            class="p-1 bg-surface text-secondary rounded-md hover:bg-surface-hover transition-colors"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0zM10 7v3m0 0v3m0-3h3m-3 0H7" />
            </svg>
          </button>
          
          <button
            @click="zoomOut"
            class="p-1 bg-surface text-secondary rounded-md hover:bg-surface-hover transition-colors"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0zM13 10H7" />
            </svg>
          </button>
        </div>
        
        <div class="text-xs text-tertiary">
          {{ formatDate(startDate) }} - {{ formatDate(endDate) }}
        </div>
      </div>
      
      <!-- Gantt Chart -->
      <div class="flex-1 overflow-x-auto bg-surface-alt rounded-lg p-2">
        <div v-if="ganttTickets.length === 0" class="flex justify-center items-center h-full">
          <div class="text-tertiary">No tickets available for Gantt view</div>
        </div>
        
        <div v-else class="gantt-chart" :class="{ dragging: isDragging }" :style="{ '--timeline-units': timeUnits.length }">
          <!-- Header with timeline -->
          <div class="gantt-header">
            <!-- Left side header (ticket info) -->
            <div class="gantt-header-left">
              <div class="gantt-header-item flex items-center">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5 text-tertiary mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4" />
                </svg>
                <span class="text-xs font-medium text-secondary">Ticket Details</span>
              </div>
            </div>
            
            <!-- Right side header (timeline) -->
            <div class="gantt-header-timeline relative">
              <!-- Today line positioned within timeline area -->
              <div 
                v-if="today >= startDate && today <= endDate"
                class="gantt-today-line-header"
                :style="{
                  left: `${((today.getTime() - startDate.getTime()) / 
                         (endDate.getTime() - startDate.getTime())) * 100}%`
                }"
              >
                <div class="today-label">Today</div>
              </div>
              
              <div 
                v-for="(unit, index) in timeUnits" 
                :key="index" 
                class="gantt-timeline-unit"
              >
                <span 
                  :class="{'text-blue-400 font-medium': isSameDay(unit, today)}"
                  class="whitespace-nowrap"
                >
                  {{ formatHeaderDate(unit) }}
                </span>
              </div>
            </div>
          </div>
          
          <!-- Gantt Body -->
          <div class="gantt-body relative">
            <div 
              v-for="ticket in ganttTickets" 
              :key="ticket.id" 
              class="gantt-row"
              @click="openTicket(ticket.id)"
            >
              <!-- Ticket info -->
              <div class="gantt-row-left">
                <!-- First row: Ticket title with ID -->
                <div class="flex items-center gap-2 w-full overflow-hidden">
                  <span class="shrink-0 text-xs text-tertiary font-mono">#{{ ticket.id }}</span>
                  <span class="ticket-title font-medium truncate text-sm text-primary">{{ ticket.title }}</span>
                </div>
                
                <!-- Second row: Status badges and assignee -->
                <div class="flex flex-wrap items-center gap-1 mt-1">
                  <!-- Status badge -->
                  <span
                    class="text-[10px] px-1.5 py-0.5 rounded-sm font-medium uppercase tracking-wide"
                    :class="{
                      'bg-blue-500/20 text-blue-400': ticket.status === 'in-progress',
                      'bg-green-500/20 text-green-400': ticket.status === 'closed',
                      'bg-yellow-500/20 text-yellow-400': ticket.status === 'open'
                    }"
                  >
                    {{ ticket.status }}
                  </span>
                  
                  <!-- Priority badge -->
                  <span
                    class="text-[10px] px-1.5 py-0.5 rounded-sm font-medium uppercase tracking-wide"
                    :class="{
                      'bg-red-500/20 text-red-400': ticket.priority === 'high',
                      'bg-yellow-500/20 text-yellow-400': ticket.priority === 'medium',
                      'bg-blue-500/20 text-blue-400': ticket.priority === 'low'
                    }"
                  >
                    {{ ticket.priority }}
                  </span>
                  
                  <!-- Assignee avatar -->
                  <div class="ml-auto" @click.stop>
                    <UserAvatar
                      v-if="ticket.assignee && ticket.assignee !== 'Unassigned'"
                      :name="ticket.assignee"
                      :avatarUrl="ticket.assignee_avatar"
                      size="xs"
                      :show-name="false"
                      :clickable="false"
                    />
                    <div v-else class="w-5 h-5 rounded-full bg-surface-hover flex items-center justify-center">
                      <svg class="w-3 h-3 text-tertiary" fill="currentColor" viewBox="0 0 20 20">
                        <path fill-rule="evenodd" d="M10 9a3 3 0 100-6 3 3 0 000 6zm-7 9a7 7 0 1114 0H3z" clip-rule="evenodd" />
                      </svg>
                    </div>
                  </div>
                </div>
              </div>
              
              <!-- Timeline with task bar -->
              <div class="gantt-row-timeline">
                <!-- Grid units that match header structure -->
                <div 
                  v-for="(unit, index) in timeUnits"
                  :key="`grid-${index}`"
                  class="gantt-row-unit"
                ></div>
                
                <!-- Today line for this row (aligned with header) -->
                <div 
                  v-if="today >= startDate && today <= endDate"
                  class="gantt-today-line-row"
                  :style="{
                    left: `${((today.getTime() - startDate.getTime()) / 
                           (endDate.getTime() - startDate.getTime())) * 100}%`
                  }"
                ></div>
                
                <!-- Task bar -->
                <div 
                  class="gantt-task-bar relative group" 
                  :style="getTaskBarStyle(ticket)"
                  @mousedown="startDrag($event, ticket, 'move')"
                  @mouseenter="showTooltip($event, ticket)"
                  @mouseleave="hideTooltip"
                  :class="{ 
                    'cursor-move': !isDragging, 
                    'cursor-grabbing': isDragging && draggedTask?.id === ticket.id,
                    'pointer-events-none': isDragging && draggedTask?.id !== ticket.id
                  }"
                >
                  <!-- Resize handle - left -->
                  <div 
                    class="absolute left-0 top-0 bottom-0 w-2 cursor-ew-resize opacity-0 group-hover:opacity-100 hover:bg-white/20"
                    @mousedown.stop="startDrag($event, ticket, 'resize-start')"
                  ></div>
                  
                  <!-- Resize handle - right -->
                  <div 
                    class="absolute right-0 top-0 bottom-0 w-2 cursor-ew-resize opacity-0 group-hover:opacity-100 hover:bg-white/20"
                    @mousedown.stop="startDrag($event, ticket, 'resize-end')"
                  ></div>
                  
                  <!-- Progress bar -->
                  <div 
                    class="gantt-task-progress" 
                    :style="{ width: `${ticket.progress}%` }"
                  ></div>
                  
                  <!-- Task label (shown on wider bars) -->
                  <div class="absolute inset-0 flex items-center px-2 overflow-hidden pointer-events-none" 
                       v-if="parseFloat(getTaskBarStyle(ticket).width.replace('%', '')) > 8">
                    <span class="text-xs text-white truncate drop-shadow-sm">
                      {{ ticket.title }}
                    </span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
    
    <!-- Tooltip -->
    <Teleport to="body">
      <div
        v-if="hoveredTask"
        class="fixed z-50 bg-surface-alt border border-default rounded-lg shadow-xl p-3 pointer-events-none"
        :style="{
          left: `${tooltipPosition.x + 10}px`,
          top: `${tooltipPosition.y - 60}px`
        }"
      >
        <div class="text-sm flex flex-col gap-1">
          <div class="font-medium text-primary">{{ hoveredTask.title }}</div>
          <div class="text-xs text-tertiary">
            <div>Start: {{ formatDate(hoveredTask.start) }}</div>
            <div>End: {{ formatDate(hoveredTask.end) }}</div>
            <div>Duration: {{ Math.ceil((hoveredTask.end - hoveredTask.start) / (1000 * 60 * 60 * 24)) }} days</div>
            <div v-if="hoveredTask.assignee" class="mt-1">
              Assignee: {{ hoveredTask.assignee }}
            </div>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.gantt-chart {
  min-width: 900px;
  min-height: 300px;
  position: relative;
}

.gantt-header {
  display: flex;
  border-bottom: 1px solid var(--border-default);
  height: 36px;
}

.gantt-header-left {
  width: 240px;
  min-width: 240px;
  border-right: 1px solid var(--border-default);
  padding: 0 12px;
  display: flex;
  align-items: center;
  position: sticky;
  left: 0;
  background-color: var(--bg-surface-alt);
  z-index: 10;
}

.gantt-header-timeline {
  flex: 1;
  display: flex;
  overflow-x: hidden;
  position: relative;
}

.gantt-timeline-unit {
  flex: 1;
  min-width: 80px;
  padding: 8px 4px;
  text-align: center;
  font-size: 11px;
  color: var(--text-tertiary);
  border-right: 1px solid var(--border-default);
  position: relative;
}

.gantt-body {
  position: relative;
}

.gantt-row {
  display: flex;
  height: 52px; /* Slightly increased from 48px to accommodate two rows */
  border-bottom: 1px solid var(--border-default);
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.gantt-row:hover {
  background-color: var(--bg-surface-hover);
}

.gantt-row-left {
  width: 240px;
  min-width: 240px;
  display: flex;
  flex-direction: column;
  justify-content: center;
  padding: 4px 12px;
  border-right: 1px solid var(--border-default);
  position: sticky;
  left: 0;
  z-index: 5;
  transition: background-color 0.15s ease;
}

.gantt-row:hover .gantt-row-left {
  background-color: var(--bg-surface-hover);
}

.gantt-row-title {
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.2;
}

.gantt-task-bar {
  position: absolute;
  height: 18px; /* Reduced from 22px */
  top: 50%;
  transform: translateY(-50%);
  border-radius: 3px;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
  cursor: pointer;
  transition: opacity 0.2s ease, transform 0.1s ease;
  user-select: none;
}

.gantt-task-bar:hover {
  opacity: 0.9;
  transform: translateY(-50%) scale(1.02);
}

.gantt-task-bar.cursor-grabbing {
  opacity: 0.8;
  z-index: 10;
}

.gantt-task-progress {
  height: 100%;
  background-color: rgba(255, 255, 255, 0.2);
  border-radius: 3px;
  pointer-events: none;
}

.gantt-today-line-header {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  background-color: #ef4444; /* red-500 */
  z-index: 15;
  pointer-events: none;
}

.gantt-today-line-row {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  background-color: #ef4444; /* red-500 */
  z-index: 10;
  pointer-events: none;
}

.gantt-today-line {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  background-color: #ef4444; /* red-500 */
  z-index: 20;
  pointer-events: none;
}

.today-label {
  position: absolute;
  top: 8px;
  left: 50%;
  transform: translateX(-50%);
  background-color: #ef4444; /* red-500 */
  color: white;
  font-size: 10px;
  padding: 2px 6px;
  border-radius: 3px;
  white-space: nowrap;
  font-weight: 500;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
}

/* Add subtle grid lines for better visualization */
.gantt-row-timeline {
  flex: 1;
  position: relative;
  overflow: hidden;
  display: flex;
  /* Remove background grid since we have perfect alignment now */
  background: transparent;
}

.gantt-row-unit {
  flex: 1;
  min-width: 80px;
  border-right: 1px solid var(--border-subtle);
  height: 100%;
}

/* Add alternating row colors for better readability */
.gantt-row:nth-child(even) {
  background-color: var(--bg-surface);
}

.gantt-row:nth-child(even) .gantt-row-left {
  background-color: var(--bg-surface);
}

.gantt-row:nth-child(even):hover,
.gantt-row:nth-child(even):hover .gantt-row-left {
  background-color: var(--bg-surface-hover);
}

/* Prevent text selection during drag */
.gantt-chart.dragging {
  user-select: none;
  -webkit-user-select: none;
  -moz-user-select: none;
  -ms-user-select: none;
}

.gantt-chart.dragging * {
  cursor: grabbing !important;
}

/* Highlight active drag */
.gantt-task-bar:active {
  filter: brightness(1.1);
}

/* Smooth transitions for non-dragging tasks */
.gantt-task-bar:not(.cursor-grabbing) {
  transition: all 0.2s ease;
}
</style> 