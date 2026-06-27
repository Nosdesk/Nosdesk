<script setup lang="ts">
/**
 * Cycle detail / Scrum board.
 *
 * Phase 8 ScrumViewShape, served as a saved-Kanban specialisation
 * scoped to one cycle: the kanban renders only the cycle's tickets,
 * with a burndown widget pinned to the toolbar so velocity stays
 * visible while the team moves cards.
 *
 * The route is `/cycles/:uuid` so a cycle's URL is shareable; the
 * per-project Cycles tab links here.
 *
 * Today the kanban groups by workflow_state.category; the secondary
 * axis can be flipped from the toolbar (status x assignee, etc.)
 * the same way it works on project detail.
 */
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useQuery, useQueryCache } from '@pinia/colada'
import { subscribe } from '@/sync/lifecycle'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useCyclesStore } from '@/stores/cycles'
import { cyclesService } from '@nosdesk/core/services/cyclesService'
import CycleBurndown from '@/components/cycles/CycleBurndown.vue'
import KanbanBoard from '@/sync/views/KanbanBoard.vue'
import AsyncBoundary from '@/components/common/AsyncBoundary.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import Modal from '@/components/Modal.vue'
import DatePicker from '@/components/common/DatePicker.vue'
import Icon from '@/components/common/Icon.vue'
import { toCardData } from '@/sync/views/cardData'
import type { CardData } from '@/sync/views/types'

const props = defineProps<{ uuid: string }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const router = useRouter()
const ticketsStore = useSyncTicketsStore()
const cyclesStore = useCyclesStore()
const queryCache = useQueryCache()

// Cache-first: the cycle metadata and its membership list are keyed on
// the uuid, so a revisit renders instantly from cache then refreshes
// silently (SWR). The kanban cards themselves come from the synced
// ticket pool below, so only these two REST shapes are cached here.
const cycleQuery = useQuery({
  key: () => ['cycle', props.uuid],
  query: () => cyclesService.get(props.uuid),
  enabled: () => !!props.uuid,
})
const ticketsQuery = useQuery({
  key: () => ['cycle', props.uuid, 'tickets'],
  query: () => cyclesService.tickets(props.uuid),
  enabled: () => !!props.uuid,
})

const cycle = computed(() => cycleQuery.data.value ?? null)
const ticketIds = computed<number[]>(() => ticketsQuery.data.value ?? [])

// Subscribe to the cycle's project so the ticket pool is populated for
// the kanban cards. Fires once the cycle resolves (we only know its
// project from the fetched metadata).
watch(
  () => cycle.value?.project_id,
  (projectId) => {
    if (projectId != null) void subscribe(`project:${projectId}`)
  },
  { immediate: true },
)

// ---- Edit cycle (name + dates) -------------------------------------
const showEdit = ref(false)
const editName = ref('')
const editStart = ref('')
const editEnd = ref('')
const savePending = ref(false)

function openEdit(): void {
  const c = cycle.value
  if (!c) return
  editName.value = c.name
  editStart.value = c.start_at ? c.start_at.slice(0, 10) : ''
  editEnd.value = c.end_at ? c.end_at.slice(0, 10) : ''
  showEdit.value = true
}

async function saveEdit(): Promise<void> {
  const name = editName.value.trim()
  if (!name || savePending.value) return
  savePending.value = true
  try {
    await cyclesStore.update(props.uuid, {
      name,
      start_at: editStart.value ? new Date(editStart.value).toISOString() : null,
      end_at: editEnd.value ? new Date(editEnd.value).toISOString() : null,
    })
    // The view reads the cycle via useQuery, not the store cache, so
    // refresh it to reflect the edit.
    await queryCache.invalidateQueries({ key: ['cycle', props.uuid] })
    showEdit.value = false
  } finally {
    savePending.value = false
  }
}

// Reserved for an upcoming "drag from outside cycle" affordance.
// The underscore prefix marks it as intentionally-unused for now.
const _ticketIdSet = computed<Set<number>>(() => new Set(ticketIds.value))

const cards = computed<CardData[]>(() => {
  const out: CardData[] = []
  for (const id of ticketIds.value) {
    const t = ticketsStore.byId(id).value
    if (!t) continue
    const card = toCardData(t)
    if (card) out.push(card)
  }
  return out
})

type SecondaryAxis = 'assignee_uuid' | 'priority'
const secondaryAxis = ref<SecondaryAxis | null>(null)

function setSecondaryAxis(axis: SecondaryAxis | null): void {
  secondaryAxis.value = axis
}

function onGroupByChange(value: string | string[]): void {
  const v = Array.isArray(value) ? value[0] : value
  setSecondaryAxis(v === '' ? null : (v as SecondaryAxis))
}

// First-load state machine for the content area; the header chrome
// renders immediately regardless (cache-first principle). A warm cache
// makes hasCycle true on entry, so the boundary never shows pending.
const loadOp = computed(() => ({
  isPending: cycleQuery.asyncStatus.value === 'loading',
  isError: cycleQuery.state.value.status === 'error',
  error: cycleQuery.error.value,
}))
const hasCycle = computed(() => cycle.value !== null)

function openCard(cardId: number): void {
  router.push(`/tickets/${cardId}`)
}

function backToCycles(): void {
  // Cycles live under their project; return to that project's Cycles
  // tab (fall back to history if the cycle hasn't loaded yet).
  const projectId = cycle.value?.project_id
  if (projectId != null) router.push(`/projects/${projectId}/cycles`)
  else router.back()
}

const stateLabel = computed<string>(() => {
  if (!cycle.value) return ''
  switch (cycle.value.state) {
    case 'planned': return t('cycle-detail-state-planned')
    case 'active': return t('cycle-detail-state-active')
    case 'completed': return t('cycle-detail-state-completed')
    default: {
      const s: string = cycle.value.state
      return s.charAt(0).toUpperCase() + s.slice(1)
    }
  }
})

const groupByOptions = computed(() => [
  { value: '', label: t('cycle-detail-group-by-status') },
  { value: 'assignee_uuid', label: t('cycle-detail-group-by-assignee') },
  { value: 'priority', label: t('cycle-detail-group-by-priority') },
])

</script>

<template>
  <div class="flex flex-col h-full min-h-0 overflow-hidden">
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div class="flex items-center gap-3 min-w-0">
        <button
          type="button"
          class="p-1.5 -ml-1.5 rounded-md text-tertiary hover:text-primary hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent transition-colors shrink-0"
          :title="$t('cycle-detail-back')"
          :aria-label="$t('cycle-detail-back')"
          @click="backToCycles"
        >
          <Icon name="chevronLeft" size="md" />
        </button>
        <div class="min-w-0">
          <h1 class="text-xl font-semibold text-primary truncate">
            {{ cycle?.name ?? $t('cycle-detail-loading-name') }}
          </h1>
          <p v-if="cycle" class="text-xs text-tertiary mt-0.5">
            {{ $t('cycle-detail-summary', { state: stateLabel, count: ticketIds.length }) }}
          </p>
        </div>
      </div>
      <div class="flex items-center gap-2 text-xs text-secondary shrink-0">
        <button
          v-if="cycle"
          type="button"
          class="p-1.5 rounded-md text-tertiary hover:text-primary hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent transition-colors shrink-0"
          :title="$t('cycle-detail-edit')"
          :aria-label="$t('cycle-detail-edit')"
          @click="openEdit"
        >
          <Icon name="documentEdit" size="sm" />
        </button>
        <span class="hidden sm:inline">{{ $t('cycle-detail-group-by-label') }}</span>
        <div class="w-40">
          <BaseDropdown
            :model-value="secondaryAxis ?? ''"
            :options="groupByOptions"
            size="sm"
            @update:model-value="onGroupByChange"
          />
        </div>
      </div>
    </header>

    <AsyncBoundary :op="loadOp" :has-data="hasCycle">
      <template #pending>
        <div class="flex-1 flex items-center justify-center text-tertiary text-sm">
          {{ $t('cycle-detail-loading') }}
        </div>
      </template>
      <template #error="{ error: boundaryError }">
        <div class="flex-1 flex items-center justify-center text-status-error text-sm">
          {{ (boundaryError as Error)?.message ?? $t('cycle-detail-error-fallback') }}
        </div>
      </template>

      <!-- Burndown is pinned above the board so it stays visible
           as the user scrolls horizontally through swimlanes. -->
      <section v-if="cycle" class="px-6 py-4 border-b border-subtle bg-surface">
        <CycleBurndown :cycle="cycle" />
      </section>

      <KanbanBoard
        class="flex-1 min-h-0"
        :cards="cards"
        :on-card-click="openCard"
        :secondary-group-by="secondaryAxis"
      />
    </AsyncBoundary>

    <Modal :show="showEdit" :title="$t('cycle-detail-edit')" size="sm" @close="showEdit = false">
      <form class="flex flex-col gap-3" @submit.prevent="saveEdit">
        <label class="flex flex-col gap-1 text-xs text-secondary">
          <span>{{ $t('project-cycles-field-name') }}</span>
          <input
            v-model="editName"
            type="text"
            :placeholder="$t('project-cycles-name-placeholder')"
            class="bg-app border border-subtle rounded-md text-sm px-2 py-1.5 text-primary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
          />
        </label>
        <div class="flex flex-col sm:flex-row gap-3">
          <label class="flex flex-col gap-1 text-xs text-secondary flex-1">
            <span>{{ $t('project-cycles-field-start') }}</span>
            <DatePicker
              v-model="editStart"
              size="md"
              block
              :aria-label="$t('project-cycles-field-start')"
            />
          </label>
          <label class="flex flex-col gap-1 text-xs text-secondary flex-1">
            <span>{{ $t('project-cycles-field-end') }}</span>
            <DatePicker
              v-model="editEnd"
              size="md"
              block
              :aria-label="$t('project-cycles-field-end')"
            />
          </label>
        </div>
      </form>
      <template #footer>
        <div class="flex justify-end gap-2">
          <button
            type="button"
            class="text-xs font-medium rounded-md px-3 py-1.5 border border-default hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            @click="showEdit = false"
          >{{ $t('project-cycles-cancel-button') }}</button>
          <button
            type="button"
            class="text-xs font-medium rounded-md px-3 py-1.5 bg-accent text-on-accent hover:opacity-90 disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            :disabled="!editName.trim() || savePending"
            @click="saveEdit"
          >{{ savePending ? $t('cycle-detail-edit-saving') : $t('cycle-detail-edit-save') }}</button>
        </div>
      </template>
    </Modal>
  </div>
</template>
