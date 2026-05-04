<script setup lang="ts">
/**
 * Project / Board route. Hosts the kanban board for a project.
 *
 * Companion routes own Gantt and Cycles; this view stays focused
 * on the board and delegates view-mode switching to ProjectTabBar.
 * The header carries page identity (project name, status, ticket
 * count); the kanban toolbar carries view-shape controls
 * (Group-by axis). Moving Group-by out of the header keeps it
 * close to the surface it affects and lets the header stay short
 * across all three tabs.
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { subscribe } from '@/sync/lifecycle'
import { useSyncProjectsStore } from '@/sync/stores/projects'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useAggregate } from '@/sync/composables'
import KanbanBoard from '@/sync/views/KanbanBoard.vue'
import ProjectTabBar from '@/components/views/ProjectTabBar.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import { toCardData } from '@/sync/views/cardData'
import type { CardData } from '@/sync/views/types'

const props = defineProps<{ id: string }>()

const router = useRouter()
const projectId = computed(() => Number(props.id))
const projectsStore = useSyncProjectsStore()
const ticketsStore = useSyncTicketsStore()

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

type SecondaryAxis = 'assignee_uuid' | 'priority'
const secondaryAxis = ref<SecondaryAxis | null>(null)

const groupByOptions = [
  { value: '', label: 'Status only' },
  { value: 'assignee_uuid', label: 'Status × Assignee' },
  { value: 'priority', label: 'Status × Priority' },
]

const groupByValue = computed<string>(() => secondaryAxis.value ?? '')

function onGroupByChange(value: string | string[]): void {
  const v = Array.isArray(value) ? value[0] : value
  secondaryAxis.value = v === '' ? null : (v as SecondaryAxis)
}
</script>

<template>
  <div class="flex flex-col h-full">
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div class="min-w-0">
        <h1 class="text-xl font-semibold text-primary truncate">
          {{ project?.name ?? 'Loading…' }}
        </h1>
        <p v-if="project" class="text-xs text-tertiary mt-0.5">
          {{ cards.length }} ticket{{ cards.length === 1 ? '' : 's' }}
        </p>
      </div>
      <span
        v-if="project"
        class="text-[10px] uppercase tracking-wide font-semibold rounded px-2 py-0.5 bg-surface-hover text-tertiary shrink-0"
      >{{ project.status }}</span>
    </header>

    <ProjectTabBar :project-id="projectId" />

    <!-- Kanban toolbar — view-shape controls live with the
         surface they affect, not in the page header. Uses the
         same BaseDropdown the rest of the app reaches for so the
         control reads as a peer of every other dropdown. -->
    <div class="flex items-center justify-end gap-2 px-6 py-2 border-b border-subtle bg-surface">
      <label class="flex items-center gap-2 text-xs text-secondary">
        <span>Group by</span>
        <div class="w-44">
          <BaseDropdown
            :model-value="groupByValue"
            :options="groupByOptions"
            size="xs"
            @update:model-value="onGroupByChange"
          />
        </div>
      </label>
    </div>

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
