<script setup lang="ts">
/**
 * Projects list — sync-engine version. Renders from the sync
 * runtime's object pool rather than a per-mount API call. Reactive
 * to live updates via the SSE outbox.
 *
 * Phase 3 scope: list grid only. Drag-to-reorder, kanban swimlane,
 * and per-project bulk actions land in Phase 4.
 */
import { onMounted, computed } from 'vue'
import { useRouter } from 'vue-router'
import { storeToRefs } from 'pinia'
import { subscribe } from '@/sync/lifecycle'
import { useSyncProjectsStore } from '@/sync/stores/projects'

const router = useRouter()
const projectsStore = useSyncProjectsStore()
const { sortedByName } = storeToRefs(projectsStore)

// Subscribe to the workspace-wide group on mount. The lifecycle
// layer is idempotent on repeat subscribes, so re-entry to this
// route during the session doesn't trigger another bootstrap fetch.
onMounted(async () => {
  await subscribe('workspace:1')
})

// Initial-load skeleton: while the bootstrap is in flight there's
// nothing in the pool yet. Render a placeholder until the first
// rows land instead of flashing an empty-state.
const isInitiallyLoading = computed(() => sortedByName.value.length === 0)

function open(id: number) {
  router.push({ name: 'project-detail', params: { id: String(id) } })
}

function statusClass(status: string) {
  switch (status.toLowerCase()) {
    case 'active':
      return 'bg-status-open-muted text-status-open border border-status-open/30'
    case 'completed':
    case 'archived':
      return 'bg-status-closed-muted text-status-closed border border-status-closed/30'
    default:
      return 'bg-surface-alt text-secondary border border-default'
  }
}
</script>

<template>
  <div class="px-4 sm:px-6 py-6 max-w-6xl mx-auto w-full">
    <header class="mb-6 flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-semibold text-primary">{{ $t('projects-list-heading') }}</h1>
        <p class="text-xs text-tertiary mt-1">{{ $t('projects-list-subheading') }}</p>
      </div>
    </header>

    <div
      v-if="isInitiallyLoading"
      class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4"
    >
      <div
        v-for="i in 6"
        :key="i"
        class="bg-surface border border-subtle rounded-lg p-4 animate-pulse"
      >
        <div class="h-5 w-2/3 bg-surface-alt rounded mb-3"></div>
        <div class="h-3 w-1/3 bg-surface-alt rounded"></div>
      </div>
    </div>

    <div
      v-else
      class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4"
    >
      <button
        v-for="project in sortedByName"
        :key="project.id"
        type="button"
        class="text-left bg-surface border border-default hover:border-strong rounded-lg p-4 transition-colors group"
        @click="open(project.id)"
      >
        <div class="flex items-center justify-between gap-2 mb-2">
          <h2 class="text-base font-medium text-primary truncate group-hover:text-accent transition-colors">
            {{ project.name }}
          </h2>
          <span
            class="text-[10px] uppercase tracking-wide font-semibold rounded px-1.5 py-0.5 flex-shrink-0"
            :class="statusClass(project.status)"
          >
            {{ project.status }}
          </span>
        </div>
        <p
          v-if="project.description"
          class="text-sm text-secondary line-clamp-2"
        >
          {{ project.description }}
        </p>
        <p v-else class="text-sm text-tertiary italic">{{ $t('projects-list-no-description') }}</p>
      </button>
    </div>
  </div>
</template>
