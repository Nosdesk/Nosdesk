<script setup lang="ts">
/**
 * Pool-backed "add existing tickets to a project" picker. Used by the
 * Gantt and Cycles views (the board uses its per-column composer
 * instead). Reads the workspace ticket set straight from the sync pool
 * — no REST, no spinner — and stays open after each add so several
 * tickets can be pulled in at once; an added ticket drops out of the
 * list as confirmation.
 */
import { ref, computed, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import Modal from '@/components/Modal.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import { useSyncTicketsStore, type SyncTicket } from '@/sync/stores/tickets'
import { paletteForColor } from '@nosdesk/core/utils/workflowColors'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'

const { $t } = useFluent()

const props = defineProps<{
  show: boolean
  /** Tickets already in the project; excluded from the list. */
  existingTicketIds?: number[]
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'add-ticket', ticketId: number): void
}>()

const ticketsStore = useSyncTicketsStore()
const searchQuery = ref('')

watch(
  () => props.show,
  (visible) => {
    if (visible) searchQuery.value = ''
  },
)

const added = computed(() => new Set(props.existingTicketIds ?? []))

/** Most-recent-first, excluding tickets already in the project, capped
 * so a large workspace doesn't render thousands of rows. Search by
 * title or #id narrows across the whole pool. */
const RESULT_CAP = 50
const results = computed<SyncTicket[]>(() => {
  const q = searchQuery.value.trim().toLowerCase()
  const out: SyncTicket[] = []
  for (const t of ticketsStore.byLastActivity) {
    if (added.value.has(t.id)) continue
    if (q && !(String(t.id) === q || t.title.toLowerCase().includes(q))) continue
    out.push(t)
    if (out.length >= RESULT_CAP) break
  }
  return out
})
</script>

<template>
  <Modal :show="show" :title="$t('project-ticket-picker-title')" @close="emit('close')" size="lg">
    <div class="flex flex-col gap-4">
      <DebouncedSearchInput
        :model-value="searchQuery"
        :placeholder="$t('project-ticket-picker-search-placeholder')"
        @update:model-value="(v: string) => (searchQuery = v)"
      />

      <EmptyState
        v-if="results.length === 0"
        icon="ticket"
        :title="searchQuery ? $t('project-ticket-picker-empty-search') : $t('project-ticket-picker-empty')"
        variant="compact"
      />

      <div v-else class="-mx-2 max-h-[50vh] overflow-y-auto">
        <button
          v-for="ticket in results"
          :key="ticket.id"
          type="button"
          class="w-full flex items-center gap-2 px-2 py-2 text-left rounded-md hover:bg-surface-hover transition-colors"
          @click="emit('add-ticket', ticket.id)"
        >
          <span
            v-if="ticket.workflow_state"
            class="inline-block w-1.5 h-1.5 rounded-full bg-current shrink-0"
            :class="paletteForColor(ticket.workflow_state.color).solid"
            aria-hidden="true"
          />
          <span class="text-xs font-mono text-tertiary tabular-nums shrink-0">#{{ ticket.id }}</span>
          <span class="text-sm text-primary truncate flex-1 min-w-0">{{ ticket.title }}</span>
          <span class="text-xs text-tertiary whitespace-nowrap shrink-0">
            {{ formatRelativeTime(ticket.last_activity_at) }}
          </span>
          <span class="text-xs font-medium text-accent shrink-0">{{ $t('project-ticket-picker-add') }}</span>
        </button>
      </div>
    </div>

    <div class="flex items-center justify-between pt-4 mt-4">
      <span class="text-xs text-tertiary">
        {{ $t('project-ticket-picker-count', { count: results.length }) }}
      </span>
      <button
        type="button"
        class="px-4 py-2 text-sm text-secondary hover:text-primary hover:bg-surface-hover rounded-lg"
        @click="emit('close')"
      >
        {{ $t('project-ticket-picker-done') }}
      </button>
    </div>
  </Modal>
</template>
