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
import { useEntity, useAggregate } from '@/sync/composables'
import KanbanBoardV2 from '@/sync/views/KanbanBoardV2.vue'
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
onMounted(async () => {
  await subscribe(`project:${projectId.value}`)
})

const project = projectsStore.byId(projectId)

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
    if (!t || !t.workflow_state) continue
    out.push({
      id: t.id,
      title: t.title,
      workflow_state: t.workflow_state,
      priority: t.priority,
      assignee_uuid: t.assignee_uuid,
      requester_uuid: t.requester_uuid,
      due_date: null,
      created_at: t.created_at,
      updated_at: t.updated_at,
      last_activity_at: t.last_activity_at,
      category_id: t.category_id,
    })
  }
  return out
})

const isLoading = computed(() => project.value == null && cards.value.length === 0)

function openCard(cardId: number): void {
  router.push(`/tickets/${cardId}`)
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
      <span
        v-if="project"
        class="text-[10px] uppercase tracking-wide font-semibold rounded px-2 py-0.5"
      >
        {{ project.status }}
      </span>
    </header>

    <div v-if="isLoading" class="flex-1 flex items-center justify-center text-tertiary">
      Loading project…
    </div>

    <KanbanBoardV2
      v-else
      class="flex-1 min-h-0"
      :cards="cards"
      :on-card-click="openCard"
    />
  </div>
</template>
