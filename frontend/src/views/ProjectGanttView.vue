<script setup lang="ts">
/**
 * Project Gantt route. Same data plumbing as ProjectDetailView's
 * kanban: subscribe to the project, materialise CardData from the
 * pool's tickets via the shared toCardData helper. Adds a one-shot
 * fetch of the project's dependency edges so the renderer can draw
 * arrows for `blocks`-typed linked_tickets entries.
 */
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { subscribe } from '@/sync/lifecycle'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useSyncProjectsStore } from '@/sync/stores/projects'
import { useCyclesStore } from '@/stores/cycles'
import { useAggregate } from '@/sync/composables'
import { dependenciesService, type DependencyEdge } from '@/services/dependenciesService'
import GanttBoard from '@/sync/views/GanttBoard.vue'
import ProjectTabBar from '@/components/views/ProjectTabBar.vue'
import { toCardData } from '@/sync/views/cardData'
import type { CardData } from '@/sync/views/types'

const props = defineProps<{ id: string }>()

const router = useRouter()
const projectId = computed(() => Number(props.id))
const projectsStore = useSyncProjectsStore()
const ticketsStore = useSyncTicketsStore()
const cyclesStore = useCyclesStore()

const project = projectsStore.byId(projectId)

// Cycles render as shaded context bands behind the bars. Sourced from
// the cycles store (cached + reactive to cycle mutations).
const cycles = computed(() => cyclesStore.cyclesForProject(projectId.value).value)

interface ProjectTicketAssoc {
  project_id: number
  ticket_id: number
  display_order: number
}

const associations = useAggregate<ProjectTicketAssoc>('project_ticket')

const cards = computed<CardData[]>(() => {
  const pid = projectId.value
  const ticketIds = associations.value
    .filter((a) => a.project_id === pid)
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

const edges = ref<DependencyEdge[]>([])

async function loadEdges(): Promise<void> {
  try {
    edges.value = await dependenciesService.forProject(projectId.value)
  } catch {
    edges.value = []
  }
}

onMounted(async () => {
  await subscribe(`project:${projectId.value}`)
  await Promise.all([loadEdges(), cyclesStore.ensureLoaded(projectId.value)])
})

watch(projectId, async () => {
  await subscribe(`project:${projectId.value}`)
  await Promise.all([loadEdges(), cyclesStore.ensureLoaded(projectId.value)])
})

function openCard(cardId: number): void {
  router.push(`/tickets/${cardId}`)
}

/** Drag-the-due-handle write-back. Optimistic via the sync store, so
 *  the bar stays where it was dropped while the patch round-trips. */
function reschedule(cardId: number, dueDate: string): void {
  void ticketsStore.patchKanbanFields(cardId, { due_date: dueDate })
}
</script>

<template>
  <div class="flex flex-col h-full">
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div class="min-w-0">
        <h1 class="text-xl font-semibold text-primary truncate">
          {{ project?.name ?? $t('project-gantt-fallback-name') }}
        </h1>
        <p class="text-xs text-tertiary mt-0.5">
          {{ $t('project-gantt-summary', { tickets: cards.length, links: edges.length }) }}
        </p>
      </div>
    </header>

    <ProjectTabBar :project-id="projectId" />

    <GanttBoard
      class="flex-1 min-h-0"
      :cards="cards"
      :edges="edges"
      :cycles="cycles"
      :on-card-click="openCard"
      :on-reschedule="reschedule"
    />
  </div>
</template>
