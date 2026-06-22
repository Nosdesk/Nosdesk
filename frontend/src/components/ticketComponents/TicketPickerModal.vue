<script setup lang="ts">
/**
 * Reusable ticket search/select modal. A debounced search over the
 * paginated ticket list, results as a compact card list, and an optional
 * "Create new ticket" affordance. Emits the chosen ticket. Used where a
 * single ticket needs picking (e.g. linking a device loan) without the
 * caller reimplementing search.
 */
import { ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'
import StatusBadge from '@/components/StatusBadge.vue'
import UserAvatar from '@/components/UserAvatar.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import Icon from '@/components/common/Icon.vue'
import ticketService from '@/services/ticketService'
import type { Ticket } from '@/types/ticket'
import { formatRelativeTime } from '@/utils/dateUtils'
import { useWorkflowStatesStore } from '@/stores/workflowStates'

const props = defineProps<{
  show: boolean
  /** Ticket ids to hide from results (already-linked, self, etc.). */
  excludeIds?: number[]
  /** Show a "Create new ticket" action that mints a blank ticket. */
  allowCreate?: boolean
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'select', ticket: { id: number; title: string }): void
}>()

const { $t } = useFluent()
const wf = useWorkflowStatesStore()

const searchQuery = ref('')
const tickets = ref<Ticket[]>([])
const loading = ref(false)
const creating = ref(false)
const error = ref<string | null>(null)

async function loadTickets() {
  loading.value = true
  error.value = null
  try {
    const response = await ticketService.getPaginatedTickets(
      { page: 1, pageSize: 20, search: searchQuery.value || undefined, sortField: 'modified', sortDirection: 'desc' },
      { requestKey: 'ticket-picker' },
    )
    const exclude = new Set(props.excludeIds ?? [])
    tickets.value = response.data.filter((tk) => !exclude.has(tk.id))
  } catch {
    error.value = $t('ticket-picker-error')
    tickets.value = []
  } finally {
    loading.value = false
  }
}

function onSearch(query: string) {
  searchQuery.value = query
  void loadTickets()
}

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
    error.value = null
    void loadTickets()
  },
)
</script>

<template>
  <Modal :show="show" :title="$t('ticket-picker-title')" size="md" @close="emit('close')">
    <div class="flex flex-col gap-3">
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

      <div class="min-h-[8rem] max-h-[50vh] overflow-y-auto -mx-1">
        <div v-if="loading" class="py-10 flex justify-center">
          <span class="inline-block animate-spin rounded-full h-5 w-5 border-b-2 border-accent" />
        </div>
        <p v-else-if="tickets.length === 0" class="py-10 text-center text-sm text-tertiary">
          {{ searchQuery ? $t('ticket-picker-empty-search') : $t('ticket-picker-empty') }}
        </p>
        <ul v-else class="flex flex-col">
          <li v-for="ticket in tickets" :key="ticket.id">
            <button
              type="button"
              class="w-full flex items-start gap-2 px-2 py-2 rounded-md text-left hover:bg-surface-hover transition-colors"
              @click="choose(ticket)"
            >
              <span class="text-xs font-mono text-tertiary mt-0.5 shrink-0">#{{ ticket.id }}</span>
              <div class="min-w-0 flex-1">
                <p class="text-sm text-primary truncate">{{ ticket.title || $t('ticket-picker-untitled') }}</p>
                <div class="flex items-center gap-2 mt-0.5">
                  <StatusBadge
                    type="status"
                    :workflow-state="wf.findById(ticket.workflow_state_id ?? -1) ?? null"
                    custom-classes="text-[11px] px-1.5 py-0.5 rounded border whitespace-nowrap"
                    :compact="true"
                  />
                  <span class="text-[11px] text-tertiary">{{ formatRelativeTime(ticket.modified) }}</span>
                </div>
              </div>
              <UserAvatar
                v-if="ticket.requester_user"
                :uuid="ticket.requester_user.uuid"
                :fallback-name="ticket.requester_user.name"
                :fallback-avatar="ticket.requester_user.avatar_thumb || ticket.requester_user.avatar_url"
                size="xs"
                :clickable="false"
                class="shrink-0"
              />
            </button>
          </li>
        </ul>
      </div>

      <div class="flex justify-end">
        <Button variant="secondary" @click="emit('close')">
          <Icon name="close" size="sm" />
          {{ $t('common-cancel') }}
        </Button>
      </div>
    </div>
  </Modal>
</template>
