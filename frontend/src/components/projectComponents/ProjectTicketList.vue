<script setup lang="ts">
import { ref, watch, watchEffect, onMounted, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import { projectService } from '@/services/projectService'
import type { TicketPriority } from '@/constants/ticketOptions'
import StatusBadge from '@/components/StatusBadge.vue'
import UserAvatar from '@/components/UserAvatar.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import MobileSearchBar from '@/components/MobileSearchBar.vue'
import { useMobileSearch } from '@/composables/useMobileSearch'
import { useThemeStore } from '@/stores/theme'
import { formatRelativeTime } from '@/utils/dateUtils'

const props = defineProps<{
  projectId: number
}>()

const emit = defineEmits<{
  'add-ticket': []
  'remove-ticket': [ticketId: number]
  'ticket-count-change': [count: number]
}>()

const router = useRouter()
const themeStore = useThemeStore()

// Mobile search integration
const {
  registerMobileSearch,
  deregisterMobileSearch,
  updateSearchQuery: updateMobileSearchQuery
} = useMobileSearch()

// State
const searchQuery = ref('')
const tickets = ref<any[]>([])
const filteredTickets = ref<any[]>([])
const loading = ref(false)
const loadingMore = ref(false)
const error = ref<string | null>(null)
const currentPage = ref(1)
const hasMore = ref(false)
const pageSize = 20

// Scroll container for infinite scroll
const scrollContainer = ref<HTMLElement | null>(null)

// Load project tickets
const loadTickets = async (page = 1, append = false) => {
  if (page === 1) {
    loading.value = true
  } else {
    loadingMore.value = true
  }
  error.value = null

  try {
    // For now, get all tickets and paginate client-side
    // TODO: Backend should support paginated project tickets
    const allTickets = await projectService.getProjectTickets(props.projectId)

    // Apply search filter
    let filtered = allTickets
    if (searchQuery.value) {
      const query = searchQuery.value.toLowerCase()
      filtered = allTickets.filter((t: any) =>
        t.id.toString().includes(query) ||
        t.title?.toLowerCase().includes(query) ||
        t.status?.toLowerCase().includes(query) ||
        t.requester?.toLowerCase().includes(query) ||
        t.assignee?.toLowerCase().includes(query)
      )
    }

    // Client-side pagination
    const startIndex = (page - 1) * pageSize
    const endIndex = startIndex + pageSize
    const pageData = filtered.slice(startIndex, endIndex)

    if (append) {
      tickets.value = [...tickets.value, ...pageData]
    } else {
      tickets.value = pageData
    }

    filteredTickets.value = tickets.value
    currentPage.value = page
    hasMore.value = endIndex < filtered.length

    emit('ticket-count-change', allTickets.length)
  } catch (err) {
    console.error('Error loading project tickets:', err)
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

// Load on mount and when projectId changes
watch(() => props.projectId, () => {
  if (props.projectId) {
    searchQuery.value = ''
    tickets.value = []
    currentPage.value = 1
    loadTickets(1, false)
  }
}, { immediate: true })

// Register mobile search on mount
onMounted(() => {
  registerMobileSearch({
    searchQuery: searchQuery.value,
    placeholder: 'Search project tickets...',
    showCreateButton: true,
    createIcon: 'ticket',
    onSearchUpdate: (value: string) => {
      handleSearch(value)
    },
    onCreate: () => {
      emit('add-ticket')
    }
  })
})

// Deregister on unmount
onUnmounted(() => {
  deregisterMobileSearch()
})

// Keep mobile search in sync
watch(searchQuery, (newVal) => {
  updateMobileSearchQuery(newVal)
})

const goToTicket = (ticketId: number) => {
  router.push(`/tickets/${ticketId}`)
}

const handleRemoveTicket = (ticketId: number, event: Event) => {
  event.stopPropagation()
  emit('remove-ticket', ticketId)
}

// Refresh method for parent to call
const refresh = () => {
  loadTickets(1, false)
}

defineExpose({ refresh })

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
  <div class="flex flex-col h-full sm:p-4 sm:gap-4">
    <!-- Desktop Header (hidden on mobile) -->
    <div class="hidden sm:flex items-center justify-between gap-4">
      <DebouncedSearchInput
        :model-value="searchQuery"
        @update:model-value="handleSearch"
        placeholder="Search tickets..."
        class="flex-1 max-w-xl"
      />
      <button
        @click="emit('add-ticket')"
        class="flex items-center gap-2 px-3 py-1.5 bg-accent text-white text-sm rounded-lg hover:opacity-90 transition-colors flex-shrink-0"
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
        </svg>
        Add Ticket
      </button>
    </div>

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
    <EmptyState
      v-else-if="!loading && tickets.length === 0"
      icon="ticket"
      :title="searchQuery ? 'No tickets match your search' : 'No tickets in this project'"
      :description="searchQuery ? 'Try a different search term' : 'Add tickets to get started with project management'"
      :action-label="searchQuery ? undefined : 'Add Ticket'"
      @action="emit('add-ticket')"
    />

    <!-- Tickets list -->
    <div
      v-else
      ref="scrollContainer"
      class="flex-1 overflow-y-auto sm:bg-surface sm:rounded-lg sm:border sm:border-default"
      @scroll="handleScroll"
    >
      <!-- Mobile: Compact list matching TicketsListView -->
      <div class="flex flex-col sm:hidden">
        <div
          v-for="(ticket, index) in tickets"
          :key="ticket.id"
          @click="goToTicket(ticket.id)"
          :class="[
            'flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer',
            index > 0 ? 'border-t border-default' : ''
          ]"
        >
          <!-- Status indicator bar -->
          <div
            v-if="themeStore.effectiveColorBlindMode"
            class="w-2 self-stretch rounded-full flex-shrink-0 relative box-border"
            :class="{
              'border-2 border-status-open bg-transparent': ticket.status === 'open',
              'border-2 border-status-in-progress bg-transparent': ticket.status === 'in-progress',
              'bg-status-closed': ticket.status === 'closed'
            }"
          >
            <div
              v-if="ticket.status === 'in-progress'"
              class="absolute inset-x-0 bottom-0 h-1/2 bg-status-in-progress rounded-b-full"
              style="left: -2px; right: -2px; bottom: -2px;"
            ></div>
          </div>
          <div
            v-else
            class="w-1.5 self-stretch rounded-full flex-shrink-0"
            :class="{
              'bg-status-open': ticket.status === 'open',
              'bg-status-in-progress': ticket.status === 'in-progress',
              'bg-status-closed': ticket.status === 'closed'
            }"
          ></div>

          <!-- Main content -->
          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-2">
              <span class="text-xs text-secondary font-medium flex-shrink-0">#{{ ticket.id }}</span>
              <span class="text-sm text-primary font-medium truncate">{{ ticket.title }}</span>
            </div>

            <div class="flex flex-wrap items-center gap-x-3 gap-y-1 mt-1.5 text-xs">
              <div class="flex items-center gap-2 flex-shrink-0">
                <StatusBadge type="status" :value="ticket.status" :short="true" :compact="true" />
                <StatusBadge type="priority" :value="ticket.priority" :short="true" :compact="true" />
              </div>

              <span class="text-tertiary flex-shrink-0">{{ formatRelativeTime(ticket.modified || ticket.updated_at) }}</span>

              <div class="flex items-center gap-1 min-w-0">
                <template v-if="ticket.assignee_user?.name || ticket.assignee">
                  <div class="flex-shrink-0 [&>div]:!w-4 [&>div]:!h-4 [&>div>*]:!w-4 [&>div>*]:!h-4 [&>div>*]:!text-[8px]">
                    <UserAvatar
                      :name="ticket.assignee_user?.uuid || ticket.assignee"
                      :userName="ticket.assignee_user?.name"
                      :avatar="ticket.assignee_user?.avatar_thumb || ticket.assignee_avatar"
                      size="xs"
                      :showName="false"
                      :clickable="false"
                    />
                  </div>
                  <span class="text-secondary truncate max-w-[100px]">{{ ticket.assignee_user?.name || ticket.assignee }}</span>
                </template>
                <span v-else class="text-tertiary italic">Unassigned</span>
              </div>
            </div>
          </div>

          <!-- Remove button -->
          <button
            @click="handleRemoveTicket(ticket.id, $event)"
            class="text-tertiary hover:text-status-error hover:bg-status-error/10 p-1.5 rounded flex-shrink-0 transition-colors"
            title="Remove from project"
          >
            <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z" clip-rule="evenodd" />
            </svg>
          </button>
        </div>
      </div>

      <!-- Desktop: Table -->
      <table class="hidden sm:table w-full">
        <thead class="bg-surface-alt text-xs text-secondary uppercase sticky top-0 z-10">
          <tr>
            <th class="px-3 py-2 text-left w-16">ID</th>
            <th class="px-3 py-2 text-left">Title</th>
            <th class="px-3 py-2 text-left w-28">Status</th>
            <th class="px-3 py-2 text-left w-20">Priority</th>
            <th class="px-3 py-2 text-left w-40">Assignee</th>
            <th class="px-3 py-2 text-left w-24">Updated</th>
            <th class="px-3 py-2 text-right w-20">Actions</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-subtle">
          <tr
            v-for="ticket in tickets"
            :key="ticket.id"
            class="hover:bg-surface-hover cursor-pointer group"
            @click="goToTicket(ticket.id)"
          >
            <td class="px-3 py-2.5">
              <span class="text-xs font-mono text-tertiary">#{{ ticket.id }}</span>
            </td>
            <td class="px-3 py-2.5">
              <span class="text-sm text-primary line-clamp-1">{{ ticket.title }}</span>
            </td>
            <td class="px-3 py-2.5">
              <StatusBadge
                type="status"
                :value="ticket.status"
                custom-classes="text-xs px-1.5 py-0.5 rounded border whitespace-nowrap"
                :compact="true"
              />
            </td>
            <td class="px-3 py-2.5">
              <span
                v-if="ticket.priority"
                class="text-xs px-1.5 py-0.5 rounded border capitalize whitespace-nowrap"
                :class="getPriorityClass(ticket.priority)"
              >
                {{ ticket.priority }}
              </span>
            </td>
            <td class="px-3 py-2.5">
              <UserAvatar
                v-if="ticket.assignee_user || ticket.assignee"
                :name="ticket.assignee_user?.uuid || ticket.assignee"
                :userName="ticket.assignee_user?.name || ticket.assignee"
                :avatar="ticket.assignee_user?.avatar_thumb || ticket.assignee_avatar"
                size="xs"
                :showName="true"
                :clickable="false"
              />
              <span v-else class="text-xs text-tertiary">Unassigned</span>
            </td>
            <td class="px-3 py-2.5">
              <span class="text-xs text-tertiary">{{ formatRelativeTime(ticket.modified || ticket.updated_at) }}</span>
            </td>
            <td class="px-3 py-2.5 text-right">
              <button
                @click="handleRemoveTicket(ticket.id, $event)"
                class="text-status-error hover:bg-status-error/20 p-1 rounded opacity-0 group-hover:opacity-100 transition-opacity"
                title="Remove from project"
              >
                <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 20 20">
                  <path fill-rule="evenodd" d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z" clip-rule="evenodd" />
                </svg>
              </button>
            </td>
          </tr>
        </tbody>
      </table>

      <!-- Loading more -->
      <div v-if="loadingMore" class="py-4 flex justify-center">
        <div class="animate-spin rounded-full h-6 w-6 border-t-2 border-b-2 border-accent"></div>
      </div>
    </div>

    <!-- Mobile Search Bar (fixed at bottom) -->
    <MobileSearchBar />
  </div>
</template>
