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
import { useProjectTicketLink } from '@/composables/useProjectTicketLink'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { logger } from '@nosdesk/core/utils/logger'
import Icon from '@/components/common/Icon.vue'
import ProjectActionsMenu from '@/components/projectComponents/ProjectActionsMenu.vue'
import AddTicketsToProjectModal from '@/components/projectComponents/AddTicketsToProjectModal.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'

const props = defineProps<{
  project: SyncProject | null
  /** Trailing meta on the right (ticket count, gantt summary, …). */
  subtitle?: string
  fallbackName?: string
  /** Show the "Add tickets" picker button. The board hides it because
   * its per-column composer is the primary add path; Gantt and Cycles
   * keep it since they have no columns to compose into. Defaults on. */
  showAddTickets?: boolean
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

// Wire the global site-header create button (its label comes from the
// route's `createButtonTextKey`) to this picker. The board hides this
// bar's own "Add tickets" button in favour of the per-column composer,
// so without this the header button would invoke nothing. Registered
// here so every project sub-view (Board / Gantt / Cycles) shares it.
usePageCreateAction(() => {
  showTicketPicker.value = true
})

const { cards: projectCards } = useProjectTickets(() => props.project?.id ?? 0)
const projectTicketIds = computed(() => projectCards.value.map((c) => c.id))
const { linkToProject } = useProjectTicketLink()

async function onAddTicket(ticketId: number): Promise<void> {
  if (!props.project) return
  // Optimistic link: the row lands in the pool immediately, so the
  // ticket drops out of the picker (it's now "in project") and shows
  // live across the views without waiting for the sync frame.
  await linkToProject(props.project.id, ticketId)
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
        class="shrink-0 text-3xs uppercase tracking-wide font-semibold rounded px-1.5 py-0.5 bg-surface-hover text-tertiary leading-none"
      >{{ $t(`project-actions-status-${project.status}`) }}</span>
      <span
        v-if="subtitle"
        class="text-xs text-tertiary tabular-nums whitespace-nowrap leading-none truncate"
      >{{ subtitle }}</span>
    </div>

    <!-- Right: project actions. -->
    <div v-if="project" class="flex items-center gap-2 shrink-0">
      <!-- Search + add existing tickets into this project. Hidden on
           the board, where the per-column composer is the add path. -->
      <button
        v-if="showAddTickets !== false"
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

    <!-- Pool-backed ticket picker. Stays open after each add so several
         tickets can be added in one go; tickets already in the project
         drop out of the list. -->
    <AddTicketsToProjectModal
      :show="showTicketPicker"
      :existing-ticket-ids="projectTicketIds"
      @close="showTicketPicker = false"
      @add-ticket="onAddTicket"
    />
  </header>
</template>
