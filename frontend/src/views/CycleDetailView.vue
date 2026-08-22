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
import BackButton from '@/components/common/BackButton.vue'
import { useViewBackFallback } from '@/router/navigation'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { subscribe } from '@/sync/lifecycle'
import { useSyncTicketsStore, type SyncTicket } from '@/sync/stores/tickets'
import { cyclesService } from '@nosdesk/core/services/cyclesService'
import * as pool from '@nosdesk/core/sync/pool'
import { findPoolCycleByUuid } from '@/composables/useProjectCycles'
import { useCycleMutations } from '@/composables/useCycleMutations'
import { useCycleStats } from '@/composables/useCycleStats'
import { useCycleBurnup } from '@/composables/useCycleBurnup'
import { isoToDateInput, dateInputToIso } from '@/utils/cycleDates'
import CycleHero from '@/components/cycles/CycleHero.vue'
import KanbanBoard from '@/sync/views/KanbanBoard.vue'
import AsyncBoundary from '@/components/common/AsyncBoundary.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import Modal from '@/components/Modal.vue'
import DatePicker from '@/components/common/DatePicker.vue'
import Icon from '@/components/common/Icon.vue'
import { toCardData } from '@/sync/views/cardData'
import type { CardData } from '@nosdesk/core/sync/views/types'

const props = defineProps<{ uuid: string }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const router = useRouter()
const ticketsStore = useSyncTicketsStore()
const mutations = useCycleMutations()

// The pool is the single read home for cycle rows; one REST get
// seeds it on cold entry (deduped by the route param), then SSE
// keeps it live. Reads are a reactive scan by uuid (the pool keys
// cycles by integer id; route params carry the uuid).
const seedPending = ref(true)
const seedError = ref<Error | null>(null)
watch(
  () => props.uuid,
  async (uuid) => {
    if (!uuid) return
    seedPending.value = true
    seedError.value = null
    try {
      const c = await cyclesService.get(uuid)
      pool.upsert('cycle', c.id, { ...c })
    } catch (e) {
      seedError.value = e instanceof Error ? e : new Error(String(e))
    } finally {
      seedPending.value = false
    }
  },
  { immediate: true },
)

const cycle = computed(() => findPoolCycleByUuid(props.uuid))

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
  editStart.value = isoToDateInput(c.start_at)
  editEnd.value = isoToDateInput(c.end_at)
  showEdit.value = true
}

async function saveEdit(): Promise<void> {
  const name = editName.value.trim()
  if (!name || savePending.value) return
  savePending.value = true
  try {
    // Mutation lands in the pool on success, so the view reflects
    // the edit immediately (no second cache to reconcile).
    await mutations.update(props.uuid, {
      name,
      start_at: dateInputToIso(editStart.value),
      end_at: dateInputToIso(editEnd.value),
    })
    showEdit.value = false
  } finally {
    savePending.value = false
  }
}

// Membership derives from the ticket pool's denormalised cycle_id
// (kept live by the backend's ticket.cycle_changed event), so a
// carryover or a move on another client relocates cards here with
// no refetch.
const memberTickets = computed<SyncTicket[]>(() => {
  const c = cycle.value
  if (!c) return []
  return ticketsStore.all().value.filter((t) => t.cycle_id === c.id)
})

const cards = computed<CardData[]>(() => {
  const out: CardData[] = []
  for (const ticket of memberTickets.value) {
    const card = toCardData(ticket)
    if (card) out.push(card)
  }
  return out
})

// Dense hero above the board: stats fold from the same pool rows
// the board renders; the burnup series stays a Colada query.
const { statsFor } = useCycleStats(memberTickets)
const heroStats = computed(() => (cycle.value ? statsFor(cycle.value) : null))
const { burnup } = useCycleBurnup(
  () => props.uuid,
  () => cycle.value?.state !== 'completed' && !!cycle.value?.start_at && !!cycle.value?.end_at,
)

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
// renders immediately regardless (cache-first principle). A warm
// pool makes hasCycle true on entry, so the boundary never shows
// pending.
const loadOp = computed(() => ({
  isPending: seedPending.value,
  isError: !!seedError.value && !cycle.value,
  error: seedError.value,
}))
const hasCycle = computed(() => cycle.value != null)

function openCard(cardId: number): void {
  router.push(`/tickets/${cardId}`)
}

// Cycles live under their project, so a deep link with no in-app history should
// land on that project's Cycles tab rather than a generic parent. `BackButton`
// pops the in-app stack first and only consults this when there is nothing to
// pop, which is the case the old hand-rolled handler approximated with
// `router.back()`.
const cyclesFallback = computed<string | undefined>(() => {
  const projectId = cycle.value?.project_id
  return projectId != null ? `/projects/${projectId}/cycles` : undefined
})

// Give the mobile header the same target the desktop button uses, so a deep
// link to a cycle still has a way back to its project.
useViewBackFallback(cyclesFallback)

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
        <BackButton icon-only :label="$t('cycle-detail-back')" :fallback-route="cyclesFallback" />
        <div class="min-w-0">
          <h1 class="text-xl font-semibold text-primary truncate">
            {{ cycle?.name ?? $t('cycle-detail-loading-name') }}
          </h1>
          <p v-if="cycle" class="text-xs text-tertiary mt-0.5">
            {{ $t('cycle-detail-summary', { state: stateLabel, count: memberTickets.length }) }}
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

      <!-- Dense hero pinned above the board so progress stays visible
           as the user scrolls horizontally through swimlanes. -->
      <section v-if="cycle && heroStats" class="px-6 py-4 border-b border-subtle bg-surface">
        <CycleHero :cycle="cycle" :stats="heroStats" :burnup="burnup" variant="dense" />
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
