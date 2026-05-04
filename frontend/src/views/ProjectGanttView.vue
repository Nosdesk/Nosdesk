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
import { useAggregate } from '@/sync/composables'
import { dependenciesService, type DependencyEdge } from '@/services/dependenciesService'
import GanttBoard from '@/sync/views/GanttBoard.vue'
import { toCardData } from '@/sync/views/cardData'
import type { CardData } from '@/sync/views/types'

const props = defineProps<{ id: string }>()

const router = useRouter()
const projectId = computed(() => Number(props.id))
const projectsStore = useSyncProjectsStore()
const ticketsStore = useSyncTicketsStore()

const project = projectsStore.byId(projectId)

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
  await loadEdges()
})

watch(projectId, async () => {
  await subscribe(`project:${projectId.value}`)
  await loadEdges()
})

function openCard(cardId: number): void {
  router.push(`/tickets/${cardId}`)
}

function backToProject(): void {
  router.push(`/projects/${projectId.value}`)
}
</script>

<template>
  <div class="flex flex-col h-full">
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div class="flex items-center gap-3 min-w-0">
        <button
          type="button"
          class="text-xs text-tertiary hover:text-primary"
          @click="backToProject"
        >‹ {{ project?.name ?? 'Project' }}</button>
        <h1 class="text-xl font-semibold text-primary">Gantt</h1>
      </div>
    </header>

    <GanttBoard
      class="flex-1 min-h-0"
      :cards="cards"
      :edges="edges"
      :on-card-click="openCard"
    />
  </div>
</template>
