<script setup lang="ts">
import { ref, watch, watchEffect } from 'vue'
import { useRouter } from 'vue-router'
import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions'
import StatusBadge from '@/components/StatusBadge.vue'
import UserAvatar from '@/components/UserAvatar.vue'
import Modal from '@/components/Modal.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import ticketService from '@/services/ticketService'
import projectService from '@/services/projectService'
import type { Ticket } from '@/types/ticket'
import { formatRelativeTime } from '@/utils/dateUtils'

const router = useRouter()

const props = defineProps<{
  show: boolean
  projectId: number
  existingTickets?: number[]
}>()

const emit = defineEmits<{
  'close': []
  'add-ticket': [ticketId: number]
  'refresh': []
}>()

// Mode toggle
const isCreatingTicket = ref(false)
const newTicketTitle = ref('New Ticket')

// Ticket list state
const searchQuery = ref('')
const tickets = ref<Ticket[]>([])
const loading = ref(false)
const loadingMore = ref(false)
const error = ref<string | null>(null)
const currentPage = ref(1)
const hasMore = ref(false)
const pageSize = 20

// Scroll container for infinite scroll
const scrollContainer = ref<HTMLElement | null>(null)

// Load tickets with pagination
const loadTickets = async (page = 1, append = false) => {
  if (page === 1) {
    loading.value = true
  } else {
    loadingMore.value = true
  }
  error.value = null

  try {
    const response = await ticketService.getPaginatedTickets({
      page,
      pageSize,
      search: searchQuery.value || undefined,
      sortField: 'modified',
      sortDirection: 'desc'
    }, `add-ticket-modal-${page}`)

    // Filter out tickets already in project
    const excludeIds = new Set(props.existingTickets || [])
    const filtered = response.data.filter(t => !excludeIds.has(t.id))

    if (append) {
      tickets.value = [...tickets.value, ...filtered]
    } else {
      tickets.value = filtered
    }

    currentPage.value = page
    hasMore.value = page < response.totalPages
  } catch (err) {
    console.error('Error loading tickets:', err)
    error.value = 'Failed to load tickets'
    if (!append) tickets.value = []
  } finally {
    loading.value = false
    loadingMore.value = false
  }
}

// Handle search changes
const handleSearch = (query: string) => {
  searchQuery.value = query
  currentPage.value = 1
  loadTickets(1, false)
}

// Infinite scroll handler
const handleScroll = (event: Event) => {
  if (!hasMore.value || loadingMore.value) return

  const target = event.target as HTMLElement
  const { scrollTop, scrollHeight, clientHeight } = target
  const distanceFromBottom = scrollHeight - scrollTop - clientHeight

  if (distanceFromBottom < 200) {
    loadTickets(currentPage.value + 1, true)
  }
}

// Auto-load more if content doesn't fill viewport
watchEffect(() => {
  const container = scrollContainer.value
  if (!container || !hasMore.value || loadingMore.value || loading.value) return
  if (tickets.value.length === 0) return

  if (container.scrollHeight <= container.clientHeight) {
    loadTickets(currentPage.value + 1, true)
  }
}, { flush: 'post' })

// Watch modal visibility
watch(() => props.show, (isOpen) => {
  if (isOpen) {
    searchQuery.value = ''
    tickets.value = []
    currentPage.value = 1
    error.value = null
    isCreatingTicket.value = false
    newTicketTitle.value = 'New Ticket'
    loadTickets(1, false)
  }
})

const addTicketToProject = async (ticketId: number) => {
  try {
    await projectService.addTicketToProject(props.projectId, ticketId)
    emit('add-ticket', ticketId)
    emit('refresh')
    emit('close')
  } catch (err) {
    console.error('Error adding ticket to project:', err)
  }
}

const viewTicket = (ticketId: number, event: Event) => {
  event.stopPropagation()
  router.push(`/tickets/${ticketId}`)
}

const createAndAddTicket = async () => {
  try {
    const newTicket = await ticketService.createTicket({
      title: newTicketTitle.value,
      status: 'open' as TicketStatus,
      priority: 'medium' as TicketPriority,
      requester: '',
      assignee: '',
      article_content: ''
    })

    await projectService.addTicketToProject(props.projectId, newTicket.id)
    emit('refresh')
    emit('close')
    router.push(`/tickets/${newTicket.id}`)
  } catch (err) {
    console.error('Error creating and adding ticket:', err)
  }
}

// Priority styling
const getPriorityClass = (priority: TicketPriority) => {
  switch (priority) {
    case 'low': return 'bg-priority-low-muted text-priority-low border-priority-low/30'
    case 'medium': return 'bg-priority-medium-muted text-priority-medium border-priority-medium/30'
    case 'high': return 'bg-priority-high-muted text-priority-high border-priority-high/30'
    default: return 'bg-surface-alt text-secondary border-default'
  }
}
</script>

<template>
  <Modal :show="show" title="Add Ticket to Project" @close="emit('close')" size="lg">
    <div class="flex flex-col gap-4 -mb-4 sm:mb-0">
      <!-- Toggle buttons -->
      <div class="flex border-b border-default -mx-4 sm:mx-0 px-4 sm:px-0">
        <button
          @click="isCreatingTicket = false"
          class="flex-1 py-2 text-center text-sm font-medium border-b-2 transition-colors"
          :class="!isCreatingTicket ? 'text-accent border-accent' : 'text-tertiary border-transparent hover:text-secondary'"
        >
          Existing Tickets
        </button>
        <button
          @click="isCreatingTicket = true"
          class="flex-1 py-2 text-center text-sm font-medium border-b-2 transition-colors"
          :class="isCreatingTicket ? 'text-accent border-accent' : 'text-tertiary border-transparent hover:text-secondary'"
        >
          Create New
        </button>
      </div>

      <!-- Create New Ticket Form -->
      <div v-if="isCreatingTicket" class="flex flex-col gap-4 py-4">
        <div>
          <label for="ticketTitle" class="block text-sm text-secondary mb-1.5">Ticket Title</label>
          <input
            id="ticketTitle"
            v-model="newTicketTitle"
            type="text"
            class="w-full px-3 py-2 rounded-lg border border-default bg-surface text-primary placeholder-tertiary focus:border-accent focus:ring-1 focus:ring-accent"
            placeholder="Enter ticket title..."
          >
        </div>

        <button
          @click="createAndAddTicket"
          class="w-full py-2.5 bg-accent hover:opacity-90 text-white font-medium rounded-lg transition-colors"
        >
          Create & Add to Project
        </button>
      </div>

      <!-- Existing Tickets View -->
      <template v-else>
        <!-- Search -->
        <DebouncedSearchInput
          :model-value="searchQuery"
          @update:model-value="handleSearch"
          placeholder="Search tickets..."
        />

        <!-- Loading state -->
        <div v-if="loading && tickets.length === 0" class="py-12 text-center text-tertiary">
          <svg class="w-6 h-6 animate-spin mx-auto mb-2" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          <span class="text-sm">Loading tickets...</span>
        </div>

        <!-- Error state -->
        <div v-else-if="error" class="py-8 text-center">
          <p class="text-status-error text-sm mb-3">{{ error }}</p>
          <button
            @click="loadTickets(1, false)"
            class="px-4 py-2 text-sm bg-accent text-white rounded-lg hover:opacity-90"
          >
            Try Again
          </button>
        </div>

        <!-- Empty state -->
        <div v-else-if="!loading && tickets.length === 0" class="py-12 text-center text-tertiary">
          <p class="text-sm">{{ searchQuery ? 'No tickets match your search' : 'No tickets available to add' }}</p>
        </div>

        <!-- Tickets list -->
        <div
          v-else
          ref="scrollContainer"
          class="-mx-4 sm:mx-0 max-h-[50vh] overflow-y-auto"
          @scroll="handleScroll"
        >
          <!-- Mobile: Card list -->
          <div class="divide-y divide-default sm:hidden">
            <div
              v-for="ticket in tickets"
              :key="ticket.id"
              class="p-4 active:bg-surface-hover cursor-pointer"
              @click="addTicketToProject(ticket.id)"
            >
              <div class="flex items-center justify-between gap-2 mb-1.5">
                <span class="text-xs font-mono text-tertiary">#{{ ticket.id }}</span>
                <div class="flex items-center gap-1 flex-nowrap">
                  <StatusBadge
                    type="status"
                    :value="ticket.status"
                    custom-classes="text-xs px-1.5 py-0.5 rounded border whitespace-nowrap"
                    :compact="true"
                  />
                  <span
                    v-if="ticket.priority"
                    class="text-xs px-1.5 py-0.5 rounded border capitalize whitespace-nowrap"
                    :class="getPriorityClass(ticket.priority)"
                  >
                    {{ ticket.priority }}
                  </span>
                </div>
              </div>

              <h4 class="text-sm font-medium text-primary line-clamp-2 mb-2">{{ ticket.title }}</h4>

              <div class="flex items-center justify-between text-xs text-tertiary">
                <UserAvatar
                  v-if="ticket.requester_user"
                  :name="ticket.requester_user.uuid"
                  :userName="ticket.requester_user.name"
                  :avatar="ticket.requester_user.avatar_thumb || ticket.requester_user.avatar_url"
                  size="xs"
                  :showName="true"
                  :clickable="false"
                />
                <span v-else>—</span>
                <span class="text-accent text-xs font-medium">Add</span>
              </div>
            </div>
          </div>

          <!-- Desktop: Table -->
          <table class="hidden sm:table w-full">
            <thead class="bg-surface-alt text-xs text-secondary uppercase sticky top-0">
              <tr>
                <th class="px-3 py-2 text-left w-14">ID</th>
                <th class="px-3 py-2 text-left">Title</th>
                <th class="px-3 py-2 text-left w-28">Status</th>
                <th class="px-3 py-2 text-left w-40">Requester</th>
                <th class="px-3 py-2 text-left w-24">Updated</th>
                <th class="px-3 py-2 text-right w-16"></th>
              </tr>
            </thead>
            <tbody class="divide-y divide-subtle">
              <tr
                v-for="ticket in tickets"
                :key="ticket.id"
                class="hover:bg-surface-hover cursor-pointer group"
                @click="addTicketToProject(ticket.id)"
              >
                <td class="px-3 py-2.5">
                  <span class="text-xs font-mono text-tertiary">#{{ ticket.id }}</span>
                </td>
                <td class="px-3 py-2.5">
                  <span class="text-sm text-primary line-clamp-1">{{ ticket.title }}</span>
                </td>
                <td class="px-3 py-2.5">
                  <div class="flex gap-1 flex-nowrap">
                    <StatusBadge
                      type="status"
                      :value="ticket.status"
                      custom-classes="text-xs px-1.5 py-0.5 rounded border whitespace-nowrap"
                      :compact="true"
                    />
                    <span
                      v-if="ticket.priority"
                      class="text-xs px-1.5 py-0.5 rounded border capitalize whitespace-nowrap"
                      :class="getPriorityClass(ticket.priority)"
                    >
                      {{ ticket.priority }}
                    </span>
                  </div>
                </td>
                <td class="px-3 py-2.5">
                  <UserAvatar
                    v-if="ticket.requester_user"
                    :name="ticket.requester_user.uuid"
                    :userName="ticket.requester_user.name"
                    :avatar="ticket.requester_user.avatar_thumb || ticket.requester_user.avatar_url"
                    size="xs"
                    :showName="true"
                    :clickable="false"
                  />
                  <span v-else class="text-xs text-tertiary">—</span>
                </td>
                <td class="px-3 py-2.5">
                  <span class="text-xs text-tertiary">{{ formatRelativeTime(ticket.modified) }}</span>
                </td>
                <td class="px-3 py-2.5 text-right">
                  <span class="text-xs text-accent font-medium opacity-0 group-hover:opacity-100 transition-opacity">Add</span>
                </td>
              </tr>
            </tbody>
          </table>

          <!-- Loading more -->
          <div v-if="loadingMore" class="py-4 text-center">
            <svg class="w-5 h-5 animate-spin mx-auto text-tertiary" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
          </div>
        </div>
      </template>
    </div>

    <!-- Footer -->
    <div class="flex items-center justify-between pt-4 mt-4">
      <span v-if="!isCreatingTicket" class="text-xs text-tertiary">
        {{ tickets.length }} ticket{{ tickets.length !== 1 ? 's' : '' }}
      </span>
      <span v-else></span>
      <button
        type="button"
        class="px-4 py-2 text-sm text-secondary hover:text-primary hover:bg-surface-hover rounded-lg"
        @click="emit('close')"
      >
        Cancel
      </button>
    </div>
  </Modal>
</template>
