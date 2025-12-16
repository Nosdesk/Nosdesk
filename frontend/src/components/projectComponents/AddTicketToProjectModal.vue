<script setup lang="ts">
import { ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions';
import StatusBadge from '@/components/StatusBadge.vue';
import Modal from '@/components/Modal.vue';
import ticketService from '@/services/ticketService';
import projectService from '@/services/projectService';
import type { Ticket } from '@/services/ticketService';

const router = useRouter();
const props = defineProps<{
  show: boolean;
  projectId: number;
  existingTickets?: number[];
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'add-ticket', ticketId: number): void;
  (e: 'refresh'): void;
}>();

const searchQuery = ref('');
const tickets = ref<Ticket[]>([]);
const filteredTickets = ref<Ticket[]>([]);
const isCreatingTicket = ref(false);
const newTicketTitle = ref('New Ticket');

// Load tickets from the API
const loadTickets = async () => {
  try {
    const allTickets = await ticketService.getTickets();
    
    // Filter out tickets that are already in the project
    tickets.value = allTickets.filter((t: Ticket) => {
      // Skip already added tickets
      if (props.existingTickets?.includes(t.id)) {
        return false;
      }
      
      return true;
    });
    
    filterTickets();
  } catch (error) {
    console.error('Error loading tickets:', error);
  }
};

// Filter tickets based on search query
const filterTickets = () => {
  if (!searchQuery.value.trim()) {
    filteredTickets.value = tickets.value;
    return;
  }
  
  const query = searchQuery.value.toLowerCase();
  filteredTickets.value = tickets.value.filter((ticket: Ticket) => 
    ticket.title.toLowerCase().includes(query) || 
    String(ticket.id).includes(query)
  );
};

// Watch for search query changes
watch(searchQuery, () => {
  filterTickets();
});

// Watch for modal visibility
watch(() => props.show, (newValue) => {
  if (newValue) {
    loadTickets();
    searchQuery.value = '';
    isCreatingTicket.value = false;
    newTicketTitle.value = 'New Ticket';
  }
});

const addTicketToProject = async (ticketId: number) => {
  try {
    await projectService.addTicketToProject(props.projectId, ticketId);
    emit('add-ticket', ticketId);
    emit('refresh');
    emit('close');
  } catch (error) {
    console.error('Error adding ticket to project:', error);
  }
};

const viewTicket = (ticketId: number, event: Event) => {
  event.stopPropagation(); // Prevent triggering the link action
  router.push(`/tickets/${ticketId}`);
};

const toggleCreateTicket = () => {
  isCreatingTicket.value = !isCreatingTicket.value;
};

const createAndAddTicket = async () => {
  try {
    // Create a new ticket
    const newTicket = await ticketService.createTicket({
      title: newTicketTitle.value,
      status: 'open' as TicketStatus,
      priority: 'medium' as TicketPriority,
      requester: '',
      assignee: '',
      article_content: ''
    });
    
    // Add the ticket to the project
    await projectService.addTicketToProject(props.projectId, newTicket.id);
    
    emit('refresh');
    emit('close');
    
    // Navigate to the new ticket
    router.push(`/tickets/${newTicket.id}`);
  } catch (error) {
    console.error('Error creating and adding ticket:', error);
  }
};
</script>

<template>
  <Modal :show="show" title="Add Ticket to Project" @close="emit('close')">
    <div class="flex flex-col gap-4">
      <!-- Toggle buttons -->
      <div class="flex mb-2">
        <button
          @click="isCreatingTicket = false"
          class="flex-1 py-2 text-center text-sm"
          :class="!isCreatingTicket ? 'text-brand-blue border-b-2 border-brand-blue' : 'text-tertiary border-b border-default'"
        >
          Existing Tickets
        </button>
        <button
          @click="isCreatingTicket = true"
          class="flex-1 py-2 text-center text-sm"
          :class="isCreatingTicket ? 'text-brand-blue border-b-2 border-brand-blue' : 'text-tertiary border-b border-default'"
        >
          Create New Ticket
        </button>
      </div>
      
      <!-- Create New Ticket Form -->
      <div v-if="isCreatingTicket" class="flex flex-col gap-4">
        <div>
          <label for="ticketTitle" class="block text-sm text-tertiary mb-1">Ticket Title</label>
          <input
            id="ticketTitle"
            v-model="newTicketTitle"
            type="text"
            class="w-full p-2 rounded-lg border-default bg-surface text-primary placeholder-tertiary focus:border-brand-blue focus:ring-brand-blue"
            placeholder="Enter ticket title..."
          >
        </div>
        
        <button
          @click="createAndAddTicket"
          class="w-full py-2 mt-2 bg-brand-blue hover:opacity-90 text-white font-medium rounded-lg transition-colors"
        >
          Create & Add to Project
        </button>
      </div>

      <!-- Existing Tickets View -->
      <div v-else class="flex flex-col gap-4">
        <!-- Search -->
        <div>
          <input type="text"
            v-model="searchQuery"
            class="w-full p-2 rounded-lg border-default bg-surface text-primary placeholder-tertiary focus:border-brand-blue focus:ring-brand-blue"
            placeholder="Search tickets by ID or title..."
          >
        </div>

        <!-- Tickets list -->
        <div class="max-h-96 overflow-y-auto">
          <div v-if="filteredTickets.length === 0" class="text-center py-4 text-tertiary">
            No tickets found
          </div>
          <div v-else class="flex flex-col gap-1">
            <div v-for="ticket in filteredTickets" :key="ticket.id"
              class="group px-2 py-1.5 rounded-lg transition-colors duration-200 hover:bg-surface-hover relative cursor-pointer flex items-center justify-between"
              @click="addTicketToProject(ticket.id)">
              <!-- Left side content -->
              <div class="flex items-center gap-3 flex-1 min-w-0">
                <StatusBadge
                  type="status"
                  :value="ticket.status"
                  custom-classes="w-2.5 h-2.5 rounded-full"
                />
                <span class="text-sm text-tertiary">#{{ ticket.id }}</span>
                <span
                  class="text-sm text-primary truncate relative"
                  :title="ticket.title"
                >
                  {{ ticket.title }}
                </span>
              </div>

              <!-- Right side content -->
              <div class="flex items-center gap-4">
                <button
                  @click="viewTicket(ticket.id, $event)"
                  class="text-tertiary hover:text-primary text-sm opacity-0 group-hover:opacity-100 transition-all">
                  View
                </button>
                <button
                  class="text-blue-400 hover:text-blue-300 text-sm">
                  Add
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Footer -->
    <div class="mt-6 flex justify-end">
      <button type="button"
        class="px-4 py-2 text-sm text-secondary hover:text-primary"
        @click="emit('close')">
        Cancel
      </button>
    </div>
  </Modal>
</template> 