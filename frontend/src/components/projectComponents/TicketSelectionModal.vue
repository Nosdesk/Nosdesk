<!-- TicketSelectionModal.vue -->
<script setup lang="ts">
import { ref, onMounted, computed, watch } from 'vue'
import Modal from '@/components/Modal.vue'
import ticketService from '@/services/ticketService'

const props = defineProps<{
  show: boolean
  projectId: number
  existingTicketIds?: number[]
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'select-ticket', ticketId: number): void
}>()

const tickets = ref<any[]>([])
const filteredTickets = ref<any[]>([])
const isLoading = ref(false)
const error = ref<string | null>(null)
const searchQuery = ref('')

// Fetch all tickets when the modal is shown
watch(() => props.show, async (isVisible) => {
  if (isVisible) {
    await fetchTickets()
  }
})

// Filter tickets based on search query
watch(searchQuery, (query) => {
  if (!query) {
    filteredTickets.value = tickets.value
    return
  }
  
  const lowerQuery = query.toLowerCase()
  filteredTickets.value = tickets.value.filter(ticket => 
    ticket.id.toString().includes(lowerQuery) || 
    ticket.title.toLowerCase().includes(lowerQuery) ||
    ticket.status.toLowerCase().includes(lowerQuery) ||
    (ticket.requester && ticket.requester.toLowerCase().includes(lowerQuery)) ||
    (ticket.assignee && ticket.assignee.toLowerCase().includes(lowerQuery))
  )
})

// Fetch all tickets
const fetchTickets = async () => {
  isLoading.value = true
  error.value = null
  
  try {
    const allTickets = await ticketService.getTickets()
    
    // Filter out tickets that are already in the project
    if (props.existingTicketIds && props.existingTicketIds.length > 0) {
      tickets.value = allTickets.filter(ticket => !props.existingTicketIds?.includes(ticket.id))
    } else {
      tickets.value = allTickets
    }
    
    filteredTickets.value = tickets.value
  } catch (err) {
    console.error('Failed to fetch tickets:', err)
    error.value = 'Failed to load tickets. Please try again later.'
  } finally {
    isLoading.value = false
  }
}

const handleSelectTicket = (ticketId: number) => {
  emit('select-ticket', ticketId)
}

const getStatusClass = (status: string) => {
  switch (status.toLowerCase()) {
    case 'open':
      return 'bg-status-open-muted text-status-open border-status-open/30'
    case 'in progress':
    case 'in-progress':
      return 'bg-status-in-progress-muted text-status-in-progress border-status-in-progress/30'
    case 'resolved':
    case 'closed':
      return 'bg-status-closed-muted text-status-closed border-status-closed/30'
    default:
      return 'bg-surface-alt text-secondary border-default'
  }
}

const getPriorityClass = (priority: string) => {
  switch (priority.toLowerCase()) {
    case 'high':
      return 'bg-priority-high-muted text-priority-high border-priority-high/30'
    case 'medium':
      return 'bg-priority-medium-muted text-priority-medium border-priority-medium/30'
    case 'low':
      return 'bg-priority-low-muted text-priority-low border-priority-low/30'
    default:
      return 'bg-surface-alt text-secondary border-default'
  }
}
</script>

<template>
  <Modal
    :show="show"
    title="Add Ticket to Project"
    @close="emit('close')"
  >
    <div class="flex flex-col gap-4">
      <!-- Search input -->
      <div class="relative">
        <input
          v-model="searchQuery"
          type="text"
          placeholder="Search tickets..."
          class="w-full px-4 py-2 bg-surface border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
        />
        <div class="absolute inset-y-0 right-0 flex items-center pr-3 pointer-events-none">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-tertiary" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M8 4a4 4 0 100 8 4 4 0 000-8zM2 8a6 6 0 1110.89 3.476l4.817 4.817a1 1 0 01-1.414 1.414l-4.816-4.816A6 6 0 012 8z" clip-rule="evenodd" />
          </svg>
        </div>
      </div>

      <!-- Error message -->
      <div v-if="error" class="bg-status-error-muted border border-status-error/30 text-status-error px-4 py-3 rounded-lg">
        {{ error }}
      </div>

      <!-- Loading state -->
      <div v-if="isLoading" class="flex justify-center items-center py-8">
        <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-accent"></div>
      </div>

      <!-- No tickets found -->
      <div v-else-if="filteredTickets.length === 0" class="text-center py-8">
        <p class="text-tertiary">No tickets found. Try a different search or create a new ticket.</p>
      </div>

      <!-- Tickets list -->
      <div v-else class="max-h-96 overflow-y-auto">
        <div class="bg-surface-alt rounded-lg overflow-hidden">
          <table class="w-full">
            <thead>
              <tr class="border-b border-default">
                <th class="px-6 py-3 text-left text-xs font-medium text-tertiary uppercase tracking-wider">ID</th>
                <th class="px-6 py-3 text-left text-xs font-medium text-tertiary uppercase tracking-wider">Title</th>
                <th class="px-6 py-3 text-left text-xs font-medium text-tertiary uppercase tracking-wider">Status</th>
                <th class="px-6 py-3 text-left text-xs font-medium text-tertiary uppercase tracking-wider">Priority</th>
                <th class="px-6 py-3 text-left text-xs font-medium text-tertiary uppercase tracking-wider">Action</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-default">
              <tr
                v-for="ticket in filteredTickets"
                :key="ticket.id"
                class="hover:bg-surface-hover transition-colors"
              >
                <td class="px-6 py-4 whitespace-nowrap text-sm text-secondary">#{{ ticket.id }}</td>
                <td class="px-6 py-4 whitespace-nowrap text-sm text-primary">{{ ticket.title }}</td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <span 
                    :class="getStatusClass(ticket.status)"
                    class="px-2 py-1 text-xs rounded-full border"
                  >
                    {{ ticket.status }}
                  </span>
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <span 
                    :class="getPriorityClass(ticket.priority)"
                    class="px-2 py-1 text-xs rounded-full border"
                  >
                    {{ ticket.priority }}
                  </span>
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <button 
                    @click="handleSelectTicket(ticket.id)"
                    class="px-3 py-1 bg-accent text-white text-xs rounded-lg hover:opacity-90 transition-colors"
                  >
                    Add
                  </button>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  </Modal>
</template> 