<script setup lang="ts">
/**
 * Shared header for a project's sub-views (Board / Gantt / Cycles) so
 * all three present an identical identity bar: inline-rename project
 * title, optional trailing meta (ticket count, etc.), status badge,
 * and the project actions menu. Per-view controls go in #actions,
 * left of the meta / status cluster.
 */
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useInlineRename } from '@/composables/useInlineRename'
import { useSyncProjectsStore, type SyncProject } from '@/sync/stores/projects'
import { logger } from '@/utils/logger'
import ProjectActionsMenu from '@/components/projectComponents/ProjectActionsMenu.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'

const props = defineProps<{
  project: SyncProject | null
  /** Trailing meta on the right (ticket count, gantt summary, …). */
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
  <header class="flex items-center gap-3 px-3 sm:px-6 h-10 shrink-0 border-b border-subtle bg-app">
    <div class="min-w-0 flex-1 flex items-center">
      <input
        v-if="editing && project"
        ref="inputEl"
        v-model="draft"
        type="text"
        class="w-full max-w-md text-sm font-semibold text-primary bg-surface-alt border border-default rounded px-2 py-0.5 leading-none focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
        :aria-label="$t('project-actions-rename')"
        @keyup.enter="done"
        @keyup.esc="cancel"
        @blur="done"
      />
      <button
        v-else-if="project"
        type="button"
        class="min-w-0 max-w-full truncate text-sm font-semibold text-primary leading-none rounded px-1 -mx-1 py-0.5 hover:bg-surface-hover transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        :title="$t('project-actions-rename')"
        @click="start(project.name)"
      >
        {{ project.name }}
      </button>
      <span v-else class="text-sm font-semibold text-primary truncate leading-none">
        {{ fallbackName ?? '' }}
      </span>
    </div>

    <div v-if="project" class="flex items-center gap-2 shrink-0">
      <slot name="actions" />
      <span
        class="text-[10px] uppercase tracking-wide font-semibold rounded px-1.5 py-0.5 bg-surface-hover text-tertiary leading-none"
      >{{ $t(`project-actions-status-${project.status}`) }}</span>
      <span
        v-if="subtitle"
        class="text-xs text-tertiary tabular-nums whitespace-nowrap leading-none"
      >{{ subtitle }}</span>
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
