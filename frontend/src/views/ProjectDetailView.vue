<script setup lang="ts">
/**
 * Project detail — sync-engine version.
 *
 * Hosts the V2 kanban board and surfaces project metadata
 * (name, status, ticket count) read from the pool. Subscribes
 * to the project's sync group on mount so the bootstrap loads
 * the project + its tickets + the workflow_states.
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { subscribe } from '@/sync/lifecycle'
import { useSyncProjectsStore } from '@/sync/stores/projects'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useAggregate } from '@/sync/composables'
import { useCyclesStore } from '@/stores/cycles'
import CycleBurndown from '@/components/cycles/CycleBurndown.vue'
import KanbanBoard from '@/sync/views/KanbanBoard.vue'
import { toCardData } from '@/sync/views/cardData'
import type { CardData } from '@/sync/views/types'

const props = defineProps<{ id: string }>()

const router = useRouter()
const projectId = computed(() => Number(props.id))
const projectsStore = useSyncProjectsStore()
const ticketsStore = useSyncTicketsStore()

// Subscribe to this project's sync group on mount. The lifecycle
// layer is idempotent — re-entry to the same route during the
// session is a no-op. Group expansion fires an incremental
// bootstrap covering the project + its tickets + project_ticket
// associations.
const cyclesStore = useCyclesStore()

onMounted(async () => {
  await subscribe(`project:${projectId.value}`)
  await cyclesStore.ensureLoaded(projectId.value)
})

const project = projectsStore.byId(projectId)
const cycles = cyclesStore.cyclesForProject(projectId.value)
const activeCycle = cyclesStore.activeCycle(projectId.value)

interface ProjectTicketAssoc {
  project_id: number
  ticket_id: number
  display_order: number
}

const associations = useAggregate<ProjectTicketAssoc>('project_ticket')

/**
 * Cards for this project, in the kanban shape. Filters the pool's
 * project_ticket associations to this project, then resolves each
 * to a Ticket via the tickets store, then maps Ticket → CardData
 * (the kanban renderer's contract).
 */
const cards = computed<CardData[]>(() => {
  const pid = projectId.value
  const ticketIds = associations.value
    .filter((a) => a.project_id === pid)
    .sort((a, b) => a.display_order - b.display_order)
    .map((a) => a.ticket_id)
  const out: CardData[] = []
  for (const id of ticketIds) {
    const t = ticketsStore.byId(id).value
    if (!t) continue
    const card = toCardData(t)
    if (card) out.push(card)
  }
  return out
})

const isLoading = computed(() => project.value == null && cards.value.length === 0)

function openCard(cardId: number): void {
  router.push(`/tickets/${cardId}`)
}

// Secondary axis for two-axis swimlanes. Local state for now —
// a follow-up persists this onto the project's saved kanban view
// (group_by.secondary in the ViewShape).
type SecondaryAxis = 'assignee_uuid' | 'priority'
const secondaryAxis = ref<SecondaryAxis | null>(null)

function setSecondaryAxis(axis: SecondaryAxis | null): void {
  secondaryAxis.value = axis
}

// ---------------------------------------------------------------
// Cycles UI: a thin "Cycles" panel toggled from the header. The
// full Group → Cycles surface lands once a Group route exists;
// this slot exposes the project's cycles and the create / promote
// / complete actions so the API is reachable while the rest of
// the surface is built out.
// ---------------------------------------------------------------
const showCyclesPanel = ref(false)
const newCycleName = ref('')
const newCycleStart = ref('')
const newCycleEnd = ref('')

async function createCycle(): Promise<void> {
  const name = newCycleName.value.trim()
  if (!name) return
  await cyclesStore.create(projectId.value, {
    name,
    start_at: newCycleStart.value ? new Date(newCycleStart.value).toISOString() : null,
    end_at: newCycleEnd.value ? new Date(newCycleEnd.value).toISOString() : null,
  })
  newCycleName.value = ''
  newCycleStart.value = ''
  newCycleEnd.value = ''
}

async function promoteToActive(uuid: string): Promise<void> {
  await cyclesStore.update(uuid, { state: 'active' })
}

async function completeCycle(uuid: string): Promise<void> {
  if (!window.confirm('Complete this cycle? The snapshot freezes once you do.')) return
  await cyclesStore.complete(uuid)
}

async function archiveCycle(uuid: string): Promise<void> {
  if (!window.confirm('Archive this cycle?')) return
  await cyclesStore.archive(uuid)
}

function formatCycleDate(iso: string | null): string {
  if (!iso) return '—'
  return new Date(iso).toLocaleDateString()
}
</script>

<template>
  <div class="flex flex-col h-full">
    <header
      class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app"
    >
      <div>
        <h1 class="text-xl font-semibold text-primary">
          {{ project?.name ?? 'Loading…' }}
        </h1>
        <p v-if="project" class="text-xs text-tertiary mt-0.5">
          {{ cards.length }} ticket{{ cards.length === 1 ? '' : 's' }}
        </p>
      </div>
      <div class="flex items-center gap-3">
        <button
          type="button"
          class="text-xs font-medium rounded-md px-2.5 py-1.5 text-secondary hover:bg-surface-hover transition-colors flex items-center gap-1.5"
          :class="{ 'bg-accent/10 text-accent': showCyclesPanel }"
          @click="showCyclesPanel = !showCyclesPanel"
        >
          Cycles
          <span
            v-if="activeCycle"
            class="text-[10px] bg-accent text-on-accent rounded px-1 py-0.5"
            :title="`Active: ${activeCycle.name}`"
          >
            {{ activeCycle.name }}
          </span>
          <span
            v-else-if="cycles.length > 0"
            class="text-[10px] text-tertiary"
          >({{ cycles.length }})</span>
        </button>
        <label class="flex items-center gap-2 text-xs text-secondary">
          <span>Group by</span>
          <select
            class="bg-surface border border-subtle rounded-md text-xs px-2 py-1 text-primary"
            :value="secondaryAxis ?? ''"
            @change="setSecondaryAxis(($event.target as HTMLSelectElement).value as 'assignee_uuid' | 'priority' | '' || null)"
          >
            <option value="">Status only</option>
            <option value="assignee_uuid">Status × Assignee</option>
            <option value="priority">Status × Priority</option>
          </select>
        </label>
        <span
          v-if="project"
          class="text-[10px] uppercase tracking-wide font-semibold rounded px-2 py-0.5"
        >
          {{ project.status }}
        </span>
      </div>
    </header>

    <!-- Cycles drawer -->
    <section
      v-if="showCyclesPanel"
      class="border-b border-subtle bg-surface px-6 py-4"
    >
      <div class="flex items-center justify-between mb-3">
        <h2 class="text-sm font-semibold text-primary">Cycles</h2>
        <button
          type="button"
          class="text-xs text-tertiary hover:text-primary"
          @click="showCyclesPanel = false"
        >Close</button>
      </div>

      <div v-if="cycles.length === 0" class="text-xs text-tertiary mb-3 italic">
        No cycles yet. Create one to start grouping tickets into iterations.
      </div>

      <CycleBurndown
        v-if="activeCycle"
        :cycle="activeCycle"
        class="mb-4"
      />

      <ul v-if="cycles.length" class="flex flex-col gap-1.5 mb-4">
        <li
          v-for="cycle in cycles"
          :key="cycle.uuid"
          class="flex items-center gap-3 rounded-md border border-subtle bg-app px-3 py-2"
        >
          <span
            class="text-[10px] uppercase tracking-wide font-semibold rounded px-1.5 py-0.5"
            :class="{
              'bg-accent text-on-accent': cycle.state === 'active',
              'bg-surface-hover text-tertiary': cycle.state === 'planned',
              'bg-surface text-tertiary opacity-70': cycle.state === 'completed',
            }"
          >{{ cycle.state }}</span>
          <span class="text-sm text-primary flex-1 truncate">{{ cycle.name }}</span>
          <span class="text-[11px] text-tertiary tabular-nums">
            {{ formatCycleDate(cycle.start_at) }} → {{ formatCycleDate(cycle.end_at) }}
          </span>
          <div class="flex items-center gap-1">
            <button
              v-if="cycle.state === 'planned'"
              type="button"
              class="text-[11px] text-secondary hover:text-primary px-1.5 py-1"
              @click="promoteToActive(cycle.uuid)"
            >Promote</button>
            <button
              v-if="cycle.state === 'active'"
              type="button"
              class="text-[11px] text-secondary hover:text-primary px-1.5 py-1"
              @click="completeCycle(cycle.uuid)"
            >Complete</button>
            <button
              v-if="cycle.state !== 'completed'"
              type="button"
              class="text-[11px] text-tertiary hover:text-primary px-1.5 py-1"
              @click="archiveCycle(cycle.uuid)"
            >Archive</button>
          </div>
        </li>
      </ul>

      <form class="flex items-end gap-2" @submit.prevent="createCycle">
        <label class="flex flex-col gap-1 text-[11px] text-tertiary flex-1">
          <span>Name</span>
          <input
            v-model="newCycleName"
            type="text"
            placeholder="e.g. Sprint 14"
            class="bg-app border border-subtle rounded-md text-sm px-2 py-1 text-primary"
          />
        </label>
        <label class="flex flex-col gap-1 text-[11px] text-tertiary">
          <span>Start</span>
          <input
            v-model="newCycleStart"
            type="date"
            class="bg-app border border-subtle rounded-md text-sm px-2 py-1 text-primary"
          />
        </label>
        <label class="flex flex-col gap-1 text-[11px] text-tertiary">
          <span>End</span>
          <input
            v-model="newCycleEnd"
            type="date"
            class="bg-app border border-subtle rounded-md text-sm px-2 py-1 text-primary"
          />
        </label>
        <button
          type="submit"
          class="text-xs font-medium rounded-md px-3 py-1.5 bg-accent text-on-accent hover:opacity-90 disabled:opacity-50"
          :disabled="newCycleName.trim().length === 0"
        >
          Create
        </button>
      </form>
    </section>

    <div v-if="isLoading" class="flex-1 flex items-center justify-center text-tertiary">
      Loading project…
    </div>

    <KanbanBoard
      v-else
      class="flex-1 min-h-0"
      :cards="cards"
      :on-card-click="openCard"
      :secondary-group-by="secondaryAxis"
    />
  </div>
</template>
