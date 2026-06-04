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
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { subscribe } from '@/sync/lifecycle'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { cyclesService, type Cycle } from '@/services/cyclesService'
import CycleBurndown from '@/components/cycles/CycleBurndown.vue'
import KanbanBoard from '@/sync/views/KanbanBoard.vue'
import { toCardData } from '@/sync/views/cardData'
import type { CardData } from '@/sync/views/types'

const props = defineProps<{ uuid: string }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const router = useRouter()
const ticketsStore = useSyncTicketsStore()

const cycle = ref<Cycle | null>(null)
const ticketIds = ref<number[]>([])
const isLoading = ref(false)
const error = ref<string | null>(null)

async function load(): Promise<void> {
  isLoading.value = true
  error.value = null
  try {
    cycle.value = await cyclesService.get(props.uuid)
    ticketIds.value = await cyclesService.tickets(props.uuid)
    // Subscribe to the cycle's project so the ticket pool is populated.
    if (cycle.value) {
      await subscribe(`project:${cycle.value.project_id}`)
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('cycle-detail-error-fallback')
  } finally {
    isLoading.value = false
  }
}

onMounted(load)
watch(() => props.uuid, load)

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
  <div class="flex flex-col h-full">
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div class="flex items-center gap-3 min-w-0">
        <button
          type="button"
          class="text-xs text-tertiary hover:text-primary"
          @click="backToCycles"
        >{{ $t('cycle-detail-back') }}</button>
        <div class="min-w-0">
          <h1 class="text-xl font-semibold text-primary truncate">
            {{ cycle?.name ?? $t('cycle-detail-loading-name') }}
          </h1>
          <p v-if="cycle" class="text-xs text-tertiary mt-0.5">
            {{ $t('cycle-detail-summary', { state: stateLabel, count: ticketIds.length }) }}
          </p>
        </div>
      </div>
      <label class="flex items-center gap-2 text-xs text-secondary">
        <span>{{ $t('cycle-detail-group-by-label') }}</span>
        <select
          class="bg-surface border border-subtle rounded-md text-xs px-2 py-1 text-primary"
          :value="secondaryAxis ?? ''"
          @change="setSecondaryAxis(($event.target as HTMLSelectElement).value as 'assignee_uuid' | 'priority' | '' || null)"
        >
          <option
            v-for="opt in groupByOptions"
            :key="opt.value"
            :value="opt.value"
          >{{ opt.label }}</option>
        </select>
      </label>
    </header>

    <div v-if="isLoading" class="flex-1 flex items-center justify-center text-tertiary text-sm">
      {{ $t('cycle-detail-loading') }}
    </div>
    <div v-else-if="error" class="flex-1 flex items-center justify-center text-status-error text-sm">
      {{ error }}
    </div>
    <template v-else-if="cycle">
      <!-- Burndown is pinned above the board so it stays visible
           as the user scrolls horizontally through swimlanes. -->
      <section class="px-6 py-4 border-b border-subtle bg-surface">
        <CycleBurndown :cycle="cycle" />
      </section>

      <KanbanBoard
        class="flex-1 min-h-0"
        :cards="cards"
        :on-card-click="openCard"
        :secondary-group-by="secondaryAxis"
      />
    </template>
  </div>
</template>
