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
import { computed, onMounted, ref, nextTick } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { subscribe } from '@/sync/lifecycle'
import { useSyncProjectsStore } from '@/sync/stores/projects'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useAggregate } from '@/sync/composables'
import KanbanBoard from '@/sync/views/KanbanBoard.vue'
import ProjectTabBar from '@/components/views/ProjectTabBar.vue'
import ProjectActionsMenu from '@/components/projectComponents/ProjectActionsMenu.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import { projectService } from '@/services/projectService'
import { logger } from '@/utils/logger'
import { toCardData } from '@/sync/views/cardData'
import type { CardData } from '@/sync/views/types'

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

// --- Project management: rename / status / delete ---------------------
const renaming = ref(false)
const draftName = ref('')
const renameInput = ref<HTMLInputElement | null>(null)
const confirmingDelete = ref(false)
const deletePending = ref(false)

async function startRename(): Promise<void> {
  draftName.value = project.value?.name ?? ''
  renaming.value = true
  await nextTick()
  renameInput.value?.focus()
  renameInput.value?.select()
}

async function commitRename(): Promise<void> {
  if (!renaming.value) return
  const next = draftName.value.trim()
  renaming.value = false
  if (next && next !== project.value?.name) {
    await projectsStore.rename(projectId.value, next)
  }
}

function cancelRename(): void {
  renaming.value = false
}

function onSetStatus(status: string): void {
  void projectsStore.setStatus(projectId.value, status)
}

async function confirmDelete(): Promise<void> {
  deletePending.value = true
  try {
    await projectService.deleteProject(projectId.value)
    router.push('/projects')
  } catch (e) {
    logger.error('Failed to delete project', e)
    deletePending.value = false
    confirmingDelete.value = false
  }
}
</script>

<template>
  <div class="flex flex-col h-full">
    <header class="flex items-center justify-between gap-3 px-6 py-4 border-b border-subtle bg-app">
      <div class="min-w-0 flex-1">
        <input
          v-if="renaming"
          ref="renameInput"
          v-model="draftName"
          type="text"
          class="text-xl font-semibold text-primary bg-surface-alt border border-default rounded px-2 py-0.5 w-full max-w-md focus:outline-none focus:border-accent"
          :aria-label="$t('project-actions-rename')"
          @keyup.enter="commitRename"
          @keyup.esc="cancelRename"
          @blur="commitRename"
        />
        <h1 v-else class="text-xl font-semibold text-primary truncate">
          {{ project?.name ?? $t('project-detail-loading-name') }}
        </h1>
        <p v-if="project" class="text-xs text-tertiary mt-0.5">
          {{ $t('project-detail-ticket-count', { count: cards.length }) }}
        </p>
      </div>
      <div v-if="project" class="flex items-center gap-2 shrink-0">
        <span
          class="text-[10px] uppercase tracking-wide font-semibold rounded px-2 py-0.5 bg-surface-hover text-tertiary"
        >{{ $t(`project-actions-status-${project.status}`) }}</span>
        <ProjectActionsMenu
          :status="project.status"
          @rename="startRename"
          @set-status="onSetStatus"
          @delete="confirmingDelete = true"
        />
      </div>
    </header>

    <ProjectTabBar :project-id="projectId" />

    <!-- Kanban toolbar — view-shape controls live with the
         surface they affect, not in the page header. Uses the
         same BaseDropdown the rest of the app reaches for so the
         control reads as a peer of every other dropdown. -->
    <div class="flex items-center justify-end gap-2 px-6 py-2 border-b border-subtle bg-surface">
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
    </div>

    <div v-if="isLoading" class="flex-1 flex items-center justify-center text-tertiary">
      {{ $t('project-detail-loading') }}
    </div>

    <KanbanBoard
      v-else
      class="flex-1 min-h-0"
      :cards="cards"
      :on-card-click="openCard"
      :secondary-group-by="secondaryAxis"
    />

    <ConfirmModal
      :show="confirmingDelete"
      variant="danger"
      :title="$t('project-delete-confirm-title')"
      :message="$t('project-delete-confirm-message', { name: project?.name ?? '' })"
      :confirm-label="$t('project-delete-confirm-button')"
      :loading="deletePending"
      @confirm="confirmDelete"
      @close="confirmingDelete = false"
    />
  </div>
</template>
