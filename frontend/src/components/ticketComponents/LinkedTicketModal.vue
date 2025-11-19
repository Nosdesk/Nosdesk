# views/components/ticketComponents/LinkedTicketModal.vue
<script setup lang="ts">
import { formatDate as formatDateUtil, formatDateTime } from '@/utils/dateUtils';
import { ref, watch, computed, nextTick } from 'vue';
import { useRouter } from 'vue-router';
import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions';
import StatusBadge from '@/components/StatusBadge.vue';
import Modal from '@/components/Modal.vue';
import ticketService from '@/services/ticketService';
import type { Ticket } from '@/services/ticketService';
import UserAvatar from '@/components/UserAvatar.vue';
import { useDataStore } from '@/stores/dataStore';

const router = useRouter();
const dataStore = useDataStore();
const props = defineProps<{
  show: boolean;
  currentTicketId: number;
  existingLinkedTickets?: number[];
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'select-ticket', ticketId: number): void;
}>();

// State management
const searchQuery = ref('');
const tickets = ref<Ticket[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);

// Search debouncing
let searchTimeout: ReturnType<typeof setTimeout> | null = null;
const searchDebounceMs = 300;

// Scroll container reference
const scrollContainer = ref<HTMLElement | null>(null);

// Load tickets from the API
const loadTickets = async () => {
  loading.value = true;
  error.value = null;
  
  try {
    const allTickets = await ticketService.getTickets();
    console.log(`Total tickets fetched: ${allTickets.length}`);
    console.log(`Current ticket ID: ${props.currentTicketId}`);
    console.log(`Existing linked tickets: ${JSON.stringify(props.existingLinkedTickets || [])}`);
    
    // Filter out the current ticket and any already linked tickets
    tickets.value = allTickets.filter((t: Ticket) => {
      // Skip the current ticket
      if (t.id === props.currentTicketId) {
        console.log(`Filtering out current ticket #${t.id}`);
        return false;
      }
      
      // Skip already linked tickets
      if (props.existingLinkedTickets?.includes(t.id)) {
        console.log(`Filtering out already linked ticket #${t.id}`);
        return false;
      }
      
      return true;
    });
    
    console.log(`Filtered out current ticket #${props.currentTicketId} and ${props.existingLinkedTickets?.length || 0} linked tickets`);
    console.log(`Displaying ${tickets.value.length} available tickets for linking`);
    
    // Pre-warm user cache for all ticket requesters to prevent individual API calls
    await preWarmUserCache();
  } catch (err) {
    console.error('Error loading tickets:', err);
    error.value = 'Failed to load tickets. Please try again.';
    tickets.value = [];
  } finally {
    loading.value = false;
  }
};

// Pre-warm user cache for all unique requester UUIDs
const preWarmUserCache = async () => {
  try {
    // Get all unique requester UUIDs
    const requesterUuids = [...new Set(
      tickets.value
        .map(ticket => ticket.requester)
        .filter(uuid => uuid && uuid.length === 36) // Filter valid UUIDs
    )];
    
    if (requesterUuids.length === 0) return;
    
    console.log(`Pre-warming user cache for ${requesterUuids.length} requesters`);
    
    // Batch load users (with a reasonable limit to avoid overwhelming the API)
    const batchSize = 10;
    for (let i = 0; i < requesterUuids.length; i += batchSize) {
      const batch = requesterUuids.slice(i, i + batchSize);
      
      // Load users in parallel for this batch
      await Promise.all(
        batch.map(async (uuid) => {
          try {
            // Check if already cached
            const cachedName = dataStore.getUserName(uuid);
            if (!cachedName) {
              await dataStore.getUserByUuid(uuid);
            }
          } catch (error) {
            console.warn(`Failed to pre-load user ${uuid}:`, error);
            // Continue with other users even if one fails
          }
        })
      );
      
      // Small delay between batches to be gentle on the API
      if (i + batchSize < requesterUuids.length) {
        await new Promise(resolve => setTimeout(resolve, 100));
      }
    }
    
    console.log('User cache pre-warming completed');
  } catch (error) {
    console.warn('Failed to pre-warm user cache:', error);
    // Don't throw - this is an optimization, not critical
  }
};

// Debounced search function
const performSearch = (query: string) => {
  if (searchTimeout) {
    clearTimeout(searchTimeout);
  }
  
  searchTimeout = setTimeout(() => {
    // Search is performed on already loaded tickets
    // No need to reload from API for client-side filtering
  }, searchDebounceMs);
};

// Filter tickets based on search query
const filteredTickets = computed(() => {
  if (!searchQuery.value.trim()) {
    return tickets.value;
  }
  
  const query = searchQuery.value.toLowerCase();
  return tickets.value.filter((ticket: Ticket) => 
    ticket.title.toLowerCase().includes(query) || 
    String(ticket.id).includes(query) ||
    ticket.article_content?.toLowerCase().includes(query)
  );
});

// Watch for search query changes
watch(searchQuery, (newQuery) => {
  performSearch(newQuery);
});

// Watch for modal visibility
watch(() => props.show, (newValue) => {
  if (newValue) {
    // Reset state
    searchQuery.value = '';
    error.value = null;
    
    // Load initial data
    nextTick(() => {
      loadTickets();
    });
  } else {
    // Clear search timeout when modal closes
    if (searchTimeout) {
      clearTimeout(searchTimeout);
      searchTimeout = null;
    }
  }
});

const selectTicket = (ticketId: number) => {
  emit('select-ticket', ticketId);
  emit('close');
};

const viewTicket = (ticketId: number, event: Event) => {
  event.stopPropagation(); // Prevent triggering the link action
  router.push(`/tickets/${ticketId}`);
};

// Get priority styling
const getPriorityClass = (priority: TicketPriority) => {
  switch (priority) {
    case 'low':
      return 'bg-green-900/30 text-green-400 border-green-700/30';
    case 'medium':
      return 'bg-yellow-900/30 text-yellow-400 border-yellow-700/30';
    case 'high':
      return 'bg-red-900/30 text-red-400 border-red-700/30';
    default:
      return 'bg-slate-700/30 text-slate-400 border-slate-600/30';
  }
};

// Format date for display
const formatDate = (dateString: string): string => {
  try {
    const date = new Date(dateString);
    const now = new Date();
    const diffTime = now.getTime() - date.getTime();
    const diffDays = Math.floor(diffTime / (1000 * 60 * 60 * 24));

    if (diffDays === 0) {
      return 'Today';
    } else if (diffDays === 1) {
      return 'Yesterday';
    } else if (diffDays < 7) {
      return `${diffDays}d ago`;
    } else {
      return formatDateUtil(dateString, 'MMM d, yyyy');
    }
  } catch (e) {
    return 'Unknown';
  }
};
</script>

<template>
  <Modal :show="show" title="Link to Another Ticket" @close="emit('close')" size="lg">
    <div class="flex flex-col gap-4">
      <!-- Search -->
      <div class="relative">
        <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
          <svg class="h-5 w-5 text-slate-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
        </div>
        <input
          type="text"
          v-model="searchQuery"
          class="w-full pl-10 pr-4 py-3 rounded-lg border border-default bg-surface text-primary placeholder-tertiary focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 transition-colors"
          placeholder="Search tickets by ID, title, or description..."
        >
        <div v-if="loading && searchQuery" class="absolute inset-y-0 right-0 pr-3 flex items-center">
          <svg class="w-5 h-5 animate-spin text-slate-400" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
        </div>
      </div>

      <!-- Loading state (initial load) -->
      <div v-if="loading && tickets.length === 0" class="text-center py-8 text-tertiary">
        <div class="inline-flex items-center gap-3">
          <svg class="w-5 h-5 animate-spin" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          <span>Loading tickets...</span>
        </div>
      </div>

      <!-- Error state -->
      <div v-else-if="error" class="text-center py-8">
        <div class="bg-red-900/20 border border-red-700/30 rounded-lg p-4">
          <p class="text-red-400 flex items-center justify-center gap-2">
            <svg class="w-5 h-5 flex-shrink-0" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
            </svg>
            {{ error }}
          </p>
          <button 
            @click="loadTickets()"
            class="mt-3 px-4 py-2 bg-red-600 text-white rounded-md hover:bg-red-700 transition-colors text-sm"
          >
            Try Again
          </button>
        </div>
      </div>

      <!-- No results -->
      <div v-else-if="!loading && filteredTickets.length === 0 && searchQuery" class="text-center py-8 text-tertiary">
        <div class="inline-flex flex-col items-center gap-3">
          <svg class="w-12 h-12 text-slate-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
          </svg>
          <div class="text-center">
            <p class="text-lg font-medium text-secondary">No tickets found</p>
            <p class="text-sm">Try adjusting your search criteria</p>
          </div>
        </div>
      </div>

      <!-- No available tickets -->
      <div v-else-if="!loading && filteredTickets.length === 0 && !searchQuery" class="text-center py-8 text-tertiary">
        <div class="inline-flex flex-col items-center gap-3">
          <svg class="w-12 h-12 text-slate-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
          </svg>
          <div class="text-center">
            <p class="text-lg font-medium text-secondary">No tickets available</p>
            <p class="text-sm">All tickets are either linked or is the current ticket</p>
          </div>
        </div>
      </div>

      <!-- Tickets list -->
      <div 
        v-else-if="filteredTickets.length > 0"
        ref="scrollContainer"
        class="max-h-[500px] overflow-y-auto"
      >
        <div class="bg-surface-alt rounded-lg border border-default overflow-hidden">
          <!-- Table header -->
          <div class="bg-surface px-4 py-3 border-b border-default sticky top-0 z-10">
            <div class="grid grid-cols-12 gap-3 text-xs font-medium text-secondary uppercase tracking-wide">
              <div class="col-span-1">ID</div>
              <div class="col-span-4">Title</div>
              <div class="col-span-2">Status & Priority</div>
              <div class="col-span-2">Requester</div>
              <div class="col-span-2">Updated</div>
              <div class="col-span-1 text-right">Actions</div>
            </div>
          </div>
          
          <!-- Ticket rows -->
          <div class="divide-y divide-subtle">
            <div
              v-for="ticket in filteredTickets"
              :key="ticket.id"
              class="group relative hover:bg-surface-hover transition-colors duration-150 cursor-pointer"
              @click="selectTicket(ticket.id)"
            >
              <div class="px-4 py-3">
                <div class="grid grid-cols-12 gap-3 items-center">
                  <!-- Ticket ID -->
                  <div class="col-span-1 min-w-0">
                    <span class="text-sm font-mono text-secondary">#{{ ticket.id }}</span>
                  </div>

                  <!-- Title -->
                  <div class="col-span-4 min-w-0">
                    <div class="flex flex-col gap-1">
                      <div class="font-medium text-primary truncate text-sm" :title="ticket.title">
                        {{ ticket.title }}
                      </div>
                      <div v-if="ticket.article_content" class="text-xs text-tertiary truncate" :title="ticket.article_content">
                        {{ ticket.article_content }}
                      </div>
                    </div>
                  </div>

                  <!-- Status & Priority -->
                  <div class="col-span-2 min-w-0">
                    <div class="flex flex-wrap gap-1">
                      <StatusBadge 
                        type="status" 
                        :value="ticket.status"
                        custom-classes="text-xs px-2 py-1 rounded-full border"
                        :compact="true"
                      />
                      <span 
                        v-if="ticket.priority"
                        class="text-xs px-2 py-1 rounded-full border"
                        :class="getPriorityClass(ticket.priority)"
                      >
                        {{ ticket.priority }}
                      </span>
                    </div>
                  </div>

                  <!-- Requester -->
                  <div class="col-span-2 min-w-0">
                    <div v-if="ticket.requester" class="flex items-center gap-2">
                      <UserAvatar 
                        :name="ticket.requester_user?.name || ticket.requester" 
                        :avatarUrl="ticket.requester_user?.avatar_thumb"
                        :userUuid="ticket.requester_user?.uuid"
                        size="sm" 
                        :show-name="true" 
                        :clickable="true"
                      />
                    </div>
                    <div v-else class="flex items-center gap-2 text-slate-500">
                      <div class="w-6 h-6 rounded-full bg-slate-700 flex items-center justify-center">
                        <svg class="w-3 h-3" fill="currentColor" viewBox="0 0 20 20">
                          <path fill-rule="evenodd" d="M10 9a3 3 0 100-6 3 3 0 000 6zm-7 9a7 7 0 1114 0H3z" clip-rule="evenodd" />
                        </svg>
                      </div>
                      <span class="text-xs">No requester</span>
                    </div>
                  </div>

                  <!-- Updated -->
                  <div class="col-span-2 min-w-0">
                    <span class="text-xs text-tertiary">{{ formatDate(ticket.modified) }}</span>
                  </div>

                  <!-- Actions -->
                  <div class="col-span-1 text-right">
                    <div class="flex items-center justify-end gap-2">
                      <button
                        @click="viewTicket(ticket.id, $event)"
                        class="text-tertiary hover:text-primary text-xs opacity-0 group-hover:opacity-100 transition-all px-2 py-1 rounded hover:bg-surface-hover"
                      >
                        View
                      </button>
                      <button 
                        class="text-blue-400 hover:text-blue-300 text-xs font-medium px-2 py-1 rounded hover:bg-blue-900/20 transition-colors"
                      >
                        Link
                      </button>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Footer -->
    <div class="mt-6 flex justify-between items-center pt-4 border-t border-default">
      <div class="flex items-center gap-2 text-sm text-tertiary">
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
        </svg>
        <span>
          {{ filteredTickets.length }} ticket{{ filteredTickets.length !== 1 ? 's' : '' }} available
        </span>
      </div>

      <button
        type="button"
        class="px-4 py-2 text-sm text-secondary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
        @click="emit('close')"
      >
        Cancel
      </button>
    </div>
  </Modal>
</template>
