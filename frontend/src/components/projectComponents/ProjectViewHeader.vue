<script setup lang="ts">
/**
 * Shared header for a project's sub-views (Board / Gantt / Cycles) so
 * all three present an identical identity bar: the project name with
 * inline rename, a status badge, a per-view subtitle, and the project
 * actions menu (rename / set status / delete). Owns the management
 * side effects, so each view drops in the header and passes only its
 * own subtitle. Per-view controls (e.g. the cycles "New cycle" button)
 * go in the #actions slot, left of the actions menu.
 */
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useInlineRename } from '@/composables/useInlineRename'
import { useSyncProjectsStore, type SyncProject } from '@/sync/stores/projects'
import { projectService } from '@/services/projectService'
import { logger } from '@/utils/logger'
import ProjectActionsMenu from '@/components/projectComponents/ProjectActionsMenu.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'

const props = defineProps<{
  project: SyncProject | null
  subtitle?: string
  fallbackName?: string
}>()

const router = useRouter()
const projectsStore = useSyncProjectsStore()

const { editing, draft, inputEl, start, done, cancel } = useInlineRename((name) => {
  const p = props.project
  if (p && name && name !== p.name) void projectsStore.rename(p.id, name)
})

function onSetStatus(status: string): void {
  if (props.project) void projectsStore.setStatus(props.project.id, status)
}

const confirmingDelete = ref(false)
const deletePending = ref(false)
async function confirmDelete(): Promise<void> {
  if (!props.project) return
  deletePending.value = true
  try {
    await projectService.deleteProject(props.project.id)
    router.push('/projects')
  } catch (e) {
    logger.error('Failed to delete project', e)
    deletePending.value = false
    confirmingDelete.value = false
  }
}
</script>

<template>
  <header class="flex items-center justify-between gap-3 px-6 py-4 border-b border-subtle bg-app">
    <div class="min-w-0 flex-1">
      <input
        v-if="editing"
        ref="inputEl"
        v-model="draft"
        type="text"
        class="w-full max-w-md text-xl font-semibold text-primary bg-surface-alt border border-default rounded px-2 py-0.5 focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
        :aria-label="$t('project-actions-rename')"
        @keyup.enter="done"
        @keyup.esc="cancel"
        @blur="done"
      />
      <h1 v-else class="text-xl font-semibold text-primary truncate">
        {{ project?.name ?? fallbackName ?? '' }}
      </h1>
      <p v-if="subtitle" class="text-xs text-tertiary mt-0.5">{{ subtitle }}</p>
    </div>

    <div v-if="project" class="flex items-center gap-2 shrink-0">
      <slot name="actions" />
      <span
        class="text-[10px] uppercase tracking-wide font-semibold rounded px-2 py-0.5 bg-surface-hover text-tertiary"
      >{{ $t(`project-actions-status-${project.status}`) }}</span>
      <ProjectActionsMenu
        :status="project.status"
        @rename="start(project.name)"
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
  </header>
</template>
