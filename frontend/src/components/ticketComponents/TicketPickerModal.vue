<script setup lang="ts">
/**
 * Reusable ticket search/select modal. Debounced search over the
 * paginated ticket list (card list on mobile, table on desktop) with
 * infinite scroll, plus an optional "Create new ticket" action. The
 * single ticket picker app-wide: linking tickets to a ticket/document,
 * and attaching a ticket to a device loan.
 */
import { ref, watch, watchEffect } from 'vue'
import { useFluent } from 'fluent-vue'
import type { TicketPriority } from '@nosdesk/core/constants/ticketOptions'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'
import StatusBadge from '@/components/StatusBadge.vue'
import UserAvatar from '@/components/UserAvatar.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import ticketService from '@/services/ticketService'
import type { Ticket } from '@nosdesk/core/types/ticket'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'
import { useWorkflowStatesStore } from '@/stores/workflowStates'

const { $t } = useFluent()
const wf = useWorkflowStatesStore()

const props = defineProps<{
  show: boolean
  /** Ticket ids to hide from results (self, already-linked, etc.). */
  excludeIds?: number[]
  /** Show a "Create new ticket" action that mints a blank ticket. */
  allowCreate?: boolean
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'select', ticket: { id: number; title: string }): void
}>()

const searchQuery = ref('')
const tickets = ref<Ticket[]>([])
const loading = ref(false)
const loadingMore = ref(false)
const creating = ref(false)
const error = ref<string | null>(null)
const currentPage = ref(1)
const hasMore = ref(false)
const pageSize = 20
const scrollContainer = ref<HTMLElement | null>(null)

async function loadTickets(page = 1, append = false) {
  if (page === 1) loading.value = true
  else loadingMore.value = true
  error.value = null
  try {
    const response = await ticketService.getPaginatedTickets(
      { page, pageSize, search: searchQuery.value || undefined, sortField: 'modified', sortDirection: 'desc' },
      { requestKey: `ticket-picker-${page}` },
    )
    const exclude = new Set(props.excludeIds ?? [])
    const filtered = response.data.filter((tk) => !exclude.has(tk.id))
    tickets.value = append ? [...tickets.value, ...filtered] : filtered
    currentPage.value = page
    hasMore.value = page < response.totalPages
  } catch {
    error.value = $t('ticket-picker-error')
    if (!append) tickets.value = []
  } finally {
    loading.value = false
    loadingMore.value = false
  }
}

function onSearch(query: string) {
  searchQuery.value = query
  currentPage.value = 1
  void loadTickets(1, false)
}

function onScroll(event: Event) {
  if (!hasMore.value || loadingMore.value) return
  const el = event.target as HTMLElement
  if (el.scrollHeight - el.scrollTop - el.clientHeight < 200) {
    void loadTickets(currentPage.value + 1, true)
  }
}

// Top up if the first page doesn't fill the scroll area.
watchEffect(() => {
  const el = scrollContainer.value
  if (!el || !hasMore.value || loadingMore.value || loading.value || tickets.value.length === 0) return
  if (el.scrollHeight <= el.clientHeight) void loadTickets(currentPage.value + 1, true)
}, { flush: 'post' })

function choose(ticket: Ticket) {
  emit('select', { id: ticket.id, title: ticket.title })
  emit('close')
}

async function createAndSelect() {
  creating.value = true
  error.value = null
  try {
    const ticket = await ticketService.createEmptyTicket()
    emit('select', { id: ticket.id, title: ticket.title })
    emit('close')
  } catch {
    error.value = $t('ticket-picker-create-failed')
  } finally {
    creating.value = false
  }
}

watch(
  () => props.show,
  (open) => {
    if (!open) return
    wf.load()
    searchQuery.value = ''
    tickets.value = []
    currentPage.value = 1
    error.value = null
    void loadTickets(1, false)
  },
)

function priorityClass(priority: TicketPriority) {
  switch (priority) {
    case 'low': return 'bg-status-success/20 text-status-success border-status-success/30'
    case 'medium': return 'bg-status-warning/20 text-status-warning border-status-warning/30'
    case 'high': return 'bg-status-error/20 text-status-error border-status-error/30'
    default: return 'bg-surface-alt text-secondary border-default'
  }
}
</script>

<template>
  <Modal :show="show" :title="$t('ticket-picker-title')" size="lg" @close="emit('close')">
    <div class="flex flex-col gap-3 -mb-4 sm:mb-0">
      <DebouncedSearchInput
        :model-value="searchQuery"
        :placeholder="$t('ticket-picker-search-placeholder')"
        @update:model-value="onSearch"
      />

      <Button
        v-if="allowCreate"
        variant="secondary"
        block
        icon="add"
        :loading="creating"
        @click="createAndSelect"
      >
        {{ $t('ticket-picker-create-new') }}
      </Button>

      <p v-if="error" class="text-sm text-status-error">{{ error }}</p>

      <div v-if="loading && tickets.length === 0" class="py-12 flex justify-center">
        <span class="inline-block animate-spin rounded-full h-6 w-6 border-b-2 border-accent" />
      </div>

      <p v-else-if="!loading && tickets.length === 0" class="py-12 text-center text-sm text-tertiary">
        {{ searchQuery ? $t('ticket-picker-empty-search') : $t('ticket-picker-empty') }}
      </p>

      <div v-else ref="scrollContainer" class="-mx-4 sm:mx-0 max-h-[50vh] overflow-y-auto" @scroll="onScroll">
        <!-- Mobile: cards -->
        <div class="divide-y divide-default sm:hidden">
          <button
            v-for="ticket in tickets"
            :key="ticket.id"
            type="button"
            class="w-full p-4 text-left active:bg-surface-hover"
            @click="choose(ticket)"
          >
            <div class="flex items-center justify-between gap-2 mb-1.5">
              <span class="text-xs font-mono text-tertiary">#{{ ticket.id }}</span>
              <div class="flex items-center gap-1 flex-nowrap">
                <StatusBadge
                  type="status"
                  :workflow-state="wf.findById(ticket.workflow_state_id ?? -1) ?? null"
                  custom-classes="text-xs px-1.5 py-0.5 rounded border whitespace-nowrap"
                  :compact="true"
                />
                <span
                  v-if="ticket.priority"
                  class="text-xs px-1.5 py-0.5 rounded border capitalize whitespace-nowrap"
                  :class="priorityClass(ticket.priority)"
                >
                  {{ ticket.priority }}
                </span>
              </div>
            </div>
            <p class="text-sm font-medium text-primary line-clamp-2">{{ ticket.title || $t('ticket-picker-untitled') }}</p>
          </button>
        </div>

        <!-- Desktop: table -->
        <table class="hidden sm:table w-full">
          <thead class="bg-surface-alt text-xs text-secondary uppercase sticky top-0">
            <tr>
              <th class="px-3 py-2 text-left w-14">{{ $t('ticket-picker-linked-col-id') }}</th>
              <th class="px-3 py-2 text-left">{{ $t('ticket-picker-linked-col-title') }}</th>
              <th class="px-3 py-2 text-left w-32">{{ $t('ticket-picker-linked-col-status') }}</th>
              <th class="px-3 py-2 text-left w-40">{{ $t('ticket-picker-linked-col-requester') }}</th>
              <th class="px-3 py-2 text-left w-24">{{ $t('ticket-picker-linked-col-updated') }}</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-subtle">
            <tr
              v-for="ticket in tickets"
              :key="ticket.id"
              class="hover:bg-surface-hover cursor-pointer"
              @click="choose(ticket)"
            >
              <td class="px-3 py-2.5"><span class="text-xs font-mono text-tertiary">#{{ ticket.id }}</span></td>
              <td class="px-3 py-2.5"><span class="text-sm text-primary line-clamp-1">{{ ticket.title || $t('ticket-picker-untitled') }}</span></td>
              <td class="px-3 py-2.5">
                <StatusBadge
                  type="status"
                  :workflow-state="wf.findById(ticket.workflow_state_id ?? -1) ?? null"
                  custom-classes="text-xs px-1.5 py-0.5 rounded border whitespace-nowrap"
                  :compact="true"
                />
              </td>
              <td class="px-3 py-2.5">
                <UserAvatar
                  v-if="ticket.requester_user"
                  :uuid="ticket.requester_user.uuid"
                  :fallback-name="ticket.requester_user.name"
                  :fallback-avatar="ticket.requester_user.avatar_thumb || ticket.requester_user.avatar_url"
                  size="xs"
                  :show-name="true"
                  :clickable="false"
                />
                <span v-else class="text-xs text-tertiary">-</span>
              </td>
              <td class="px-3 py-2.5"><span class="text-xs text-tertiary">{{ formatRelativeTime(ticket.modified) }}</span></td>
            </tr>
          </tbody>
        </table>

        <div v-if="loadingMore" class="py-4 flex justify-center">
          <span class="inline-block animate-spin rounded-full h-5 w-5 border-b-2 border-accent" />
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex items-center justify-between">
        <span class="text-xs text-tertiary">{{ $t('ticket-picker-linked-count', { count: tickets.length }) }}</span>
        <Button variant="secondary" @click="emit('close')">{{ $t('common-cancel') }}</Button>
      </div>
    </template>
  </Modal>
</template>
