<script setup lang="ts">
/**
 * Shared controls bar for a project's sub-views (Board / Gantt / Cycles).
 * The project name itself lives in the main site header (PageHeader),
 * inline-editable there the same way a ticket title is (see the
 * useTitleManager wiring below); this bar carries the project meta
 * (status, ticket count), the add-tickets action, per-view #actions, and
 * the project actions menu.
 */
import { computed, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useTitleManager } from '@/composables/useTitleManager'
import { useSyncProjectsStore, type SyncProject } from '@/sync/stores/projects'
import { useProjectTickets } from '@/composables/useProjectTickets'
import projectService from '@/services/projectService'
import { logger } from '@/utils/logger'
import Icon from '@/components/common/Icon.vue'
import ProjectActionsMenu from '@/components/projectComponents/ProjectActionsMenu.vue'
import LinkedTicketModal from '@/components/ticketComponents/LinkedTicketModal.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'

const props = defineProps<{
  project: SyncProject | null
  /** Trailing meta on the right (ticket count, gantt summary, …). */
  subtitle?: string
  fallbackName?: string
}>()

const router = useRouter()
const projectsStore = useSyncProjectsStore()

// Surface the project name in the main site header (PageHeader) and make
// it inline-editable there, exactly like the ticket view. Custom title
// wins over the route title in useTitleManager, so it shows on Board /
// Gantt / Cycles alike; the save handler renames the project. Both are
// cleared on leave so they don't linger onto the next route.
const titleManager = useTitleManager()
watch(
  () => props.project?.name,
  (name) => {
    if (name) titleManager.setCustomTitle(name)
  },
  { immediate: true },
)
titleManager.onCustomTitleSave(async (name: string) => {
  const p = props.project
  if (p && name && name !== p.name) await projectsStore.rename(p.id, name)
})
onUnmounted(() => {
  titleManager.setCustomTitle(null)
  titleManager.onCustomTitleSave(null)
})

function onSetStatus(status: string): void {
  if (props.project) void projectsStore.setStatus(props.project.id, status)
}

// Search-and-add existing tickets into this project. Lives in the shared
// header so it's available identically on all three views (Board / Gantt
// / Cycles). Reuses the workspace ticket picker; the add emits a
// ProjectTicket sync event, so the ticket appears live in every view
// without a refetch. The picker stays open for adding several at once.
const showTicketPicker = ref(false)
const { cards: projectCards } = useProjectTickets(() => props.project?.id ?? 0)
const projectTicketIds = computed(() => projectCards.value.map((c) => c.id))

async function onAddTicket(ticketId: number): Promise<void> {
  if (!props.project) return
  try {
    await projectService.addTicketToProject(props.project.id, ticketId)
  } catch (e) {
    logger.error('Failed to add ticket to project', e)
  }
}

const confirmingDelete = ref(false)
const deletePending = ref(false)
async function confirmDelete(): Promise<void> {
  if (!props.project) return
  deletePending.value = true
  try {
    await projectsStore.remove(props.project.id)
    router.push('/projects')
  } catch (e) {
    logger.error('Failed to delete project', e)
    deletePending.value = false
    confirmingDelete.value = false
  }
}
</script>

<template>
  <header class="flex items-center justify-between gap-3 px-3 sm:px-6 h-10 shrink-0 border-b border-subtle bg-app">
    <!-- Left: project meta. The project name itself lives in the main
         site header (PageHeader), inline-editable there. -->
    <div v-if="project" class="flex items-center gap-2 min-w-0">
      <span
        class="shrink-0 text-[10px] uppercase tracking-wide font-semibold rounded px-1.5 py-0.5 bg-surface-hover text-tertiary leading-none"
      >{{ $t(`project-actions-status-${project.status}`) }}</span>
      <span
        v-if="subtitle"
        class="text-xs text-tertiary tabular-nums whitespace-nowrap leading-none truncate"
      >{{ subtitle }}</span>
    </div>

    <!-- Right: project actions. -->
    <div v-if="project" class="flex items-center gap-2 shrink-0">
      <!-- Search + add existing tickets into this project (all 3 views). -->
      <button
        type="button"
        class="inline-flex items-center gap-1 text-xs font-medium text-secondary hover:text-primary hover:bg-surface-hover rounded-md px-2 py-1 border border-subtle focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        @click="showTicketPicker = true"
      >
        <Icon name="add" />
        <span class="hidden sm:inline">{{ $t('project-add-tickets') }}</span>
      </button>
      <slot name="actions" />
      <ProjectActionsMenu
        :status="project.status"
        @set-status="onSetStatus"
        @delete="confirmingDelete = true"
      />
    </div>

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

    <!-- Workspace ticket picker. Stays open after each pick so several
         tickets can be added in one go; tickets already in the project
         are filtered out. -->
    <LinkedTicketModal
      :show="showTicketPicker"
      :current-ticket-id="0"
      :existing-linked-tickets="projectTicketIds"
      @close="showTicketPicker = false"
      @select-ticket="onAddTicket"
    />
  </header>
</template>
