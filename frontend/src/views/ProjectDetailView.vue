<script setup lang="ts">
/**
 * Project / Board route. Hosts the kanban board for a project.
 *
 * Companion routes own Gantt and Cycles; this view stays focused
 * on the board and delegates view-mode switching to ProjectTabBar.
 * The header carries page identity (editable project name, status,
 * ticket count on the right); the kanban toolbar carries view-shape controls
 * (Group-by axis). Moving Group-by out of the header keeps it
 * close to the surface it affects and lets the header stay short
 * across all three tabs.
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { subscribe } from '@/sync/lifecycle'
import * as pool from '@/sync/pool'
import { useSyncProjectsStore } from '@/sync/stores/projects'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useProjectTickets } from '@/composables/useProjectTickets'
import KanbanBoard from '@/sync/views/KanbanBoard.vue'
import ProjectTabBar from '@/components/views/ProjectTabBar.vue'
import ProjectViewHeader from '@/components/projectComponents/ProjectViewHeader.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import { projectService } from '@/services/projectService'
import { logger } from '@/utils/logger'

const props = defineProps<{ id: string }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const router = useRouter()
const projectId = computed(() => Number(props.id))
const projectsStore = useSyncProjectsStore()
const ticketsStore = useSyncTicketsStore()

onMounted(async () => {
  await subscribe(`project:${projectId.value}`)
})

const project = projectsStore.byId(projectId)
const { cards } = useProjectTickets(projectId)

const isLoading = computed(() => project.value == null && cards.value.length === 0)

function openCard(cardId: number): void {
  router.push(`/tickets/${cardId}`)
}

async function quickAdd(workflowStateId: number, title: string): Promise<void> {
  try {
    const created = await projectService.createTicketInProject(projectId.value, {
      title,
      workflow_state_id: workflowStateId,
    })
    // The ticket.created / project_ticket.added events also arrive over
    // the project:<id> sync stream, but optimistically seeding the pool
    // here makes the card appear instantly rather than after the SSE
    // round-trip. ensureInPool fetches the full row (nested
    // workflow_state included) so it renders; the link upsert puts it
    // in this project's view. Both are idempotent with the SSE frame.
    const newId = created?.id
    if (typeof newId === 'number') {
      await ticketsStore.ensureInPool(newId)
      pool.upsert('project_ticket', `${projectId.value}:${newId}`, {
        project_id: projectId.value,
        ticket_id: newId,
        display_order: 0,
      })
    }
  } catch (e) {
    logger.error('Quick-add failed', e)
  }
}

type SecondaryAxis = 'assignee_uuid' | 'priority'
const secondaryAxis = ref<SecondaryAxis | null>(null)

const groupByOptions = computed(() => [
  { value: '', label: t('project-detail-group-by-status') },
  { value: 'assignee_uuid', label: t('project-detail-group-by-assignee') },
  { value: 'priority', label: t('project-detail-group-by-priority') },
])

const groupByValue = computed<string>(() => secondaryAxis.value ?? '')

function onGroupByChange(value: string | string[]): void {
  const v = Array.isArray(value) ? value[0] : value
  secondaryAxis.value = v === '' ? null : (v as SecondaryAxis)
}
</script>

<template>
  <div class="flex flex-col h-full min-h-0 overflow-hidden">
    <ProjectViewHeader
      :project="project"
      :subtitle="project ? $t('project-detail-ticket-count', { count: cards.length }) : undefined"
      :fallback-name="$t('project-detail-loading-name')"
      :show-add-tickets="false"
    />

    <!-- View-shape controls (Group-by) ride the tab-bar row, kept
         close to the surface they affect rather than in the header.
         BaseDropdown is the same control the rest of the app uses, so
         it reads as a peer of every other dropdown. -->
    <ProjectTabBar :project-id="projectId">
      <template #actions>
        <label class="flex items-center gap-2 text-xs text-secondary">
          <span>{{ $t('project-detail-group-by-label') }}</span>
          <div class="w-44">
            <BaseDropdown
              :model-value="groupByValue"
              :options="groupByOptions"
              size="xs"
              @update:model-value="onGroupByChange"
            />
          </div>
        </label>
      </template>
    </ProjectTabBar>

    <div v-if="isLoading" class="flex-1 flex items-center justify-center text-tertiary">
      {{ $t('project-detail-loading') }}
    </div>

    <KanbanBoard
      v-else
      class="flex-1 min-h-0"
      :project-id="projectId"
      :cards="cards"
      :on-card-click="openCard"
      :on-quick-add="quickAdd"
      :secondary-group-by="secondaryAxis"
    />
  </div>
</template>
