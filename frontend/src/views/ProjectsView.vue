<script setup lang="ts">
/**
 * Projects list — sync-engine version. Renders from the sync runtime's
 * object pool (live via the SSE outbox), with search + status filter,
 * and per-card manage actions (rename / status / delete) that reuse the
 * same ProjectActionsMenu the detail header uses.
 */
import { onMounted, computed, ref, type ComponentPublicInstance } from 'vue'
import { useRouter } from 'vue-router'
import { storeToRefs } from 'pinia'
import { useFluent } from 'fluent-vue'
import { subscribe } from '@/sync/lifecycle'
import { useSyncProjectsStore, type SyncProject } from '@/sync/stores/projects'
import { useAggregate } from '@/sync/composables'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { formatRelativeTime } from '@/utils/dateUtils'
import { projectService } from '@/services/projectService'
import { logger } from '@/utils/logger'
import CreateProjectModal from '@/components/projectComponents/CreateProjectModal.vue'
import ProjectActionsMenu from '@/components/projectComponents/ProjectActionsMenu.vue'
import Button from '@/components/common/Button.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import UserAvatar from '@/components/UserAvatar.vue'

const router = useRouter()
const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const projectsStore = useSyncProjectsStore()
const { sortedByName } = storeToRefs(projectsStore)

const bootstrapped = ref(false)
const createOpen = ref(false)

onMounted(async () => {
  await subscribe('workspace:1')
  bootstrapped.value = true
})

usePageCreateAction(() => {
  createOpen.value = true
})

// Live ticket counts from the association pool (same aggregate the board
// reads), so a card's count tracks tickets being linked/unlinked.
interface ProjectTicketAssoc {
  project_id: number
  ticket_id: number
}
const associations = useAggregate<ProjectTicketAssoc>('project_ticket')
const ticketCounts = computed(() => {
  const counts = new Map<number, number>()
  for (const a of associations.value) {
    counts.set(a.project_id, (counts.get(a.project_id) ?? 0) + 1)
  }
  return counts
})
const ticketCount = (id: number): number => ticketCounts.value.get(id) ?? 0

// Search + status filter.
const search = ref('')
const statusFilter = ref('all')
const statusFilterOptions = computed(() => [
  { value: 'all', label: t('projects-filter-status-all') },
  { value: 'active', label: t('project-actions-status-active') },
  { value: 'completed', label: t('project-actions-status-completed') },
  { value: 'archived', label: t('project-actions-status-archived') },
])
const filtered = computed(() => {
  const q = search.value.trim().toLowerCase()
  return sortedByName.value.filter(
    (p) =>
      (statusFilter.value === 'all' || p.status === statusFilter.value) &&
      (q === '' || p.name.toLowerCase().includes(q)),
  )
})

const isInitiallyLoading = computed(() => !bootstrapped.value)
const isEmpty = computed(() => bootstrapped.value && sortedByName.value.length === 0)
const noMatches = computed(
  () => bootstrapped.value && sortedByName.value.length > 0 && filtered.value.length === 0,
)

function open(id: number): void {
  router.push({ name: 'project-detail', params: { id: String(id) } })
}

function onStatusFilter(value: string | string[]): void {
  statusFilter.value = Array.isArray(value) ? value[0] : value
}

function statusDot(status: string): string {
  if (status === 'active') return 'bg-status-open'
  if (status === 'completed') return 'bg-status-success'
  return 'bg-tertiary'
}

// Inline rename. Only one card edits at a time, so the function ref on the
// freshly-mounted input focuses + selects it.
const editingId = ref<number | null>(null)
const draftName = ref('')
function focusRename(el: Element | ComponentPublicInstance | null): void {
  if (el instanceof HTMLInputElement) {
    el.focus()
    el.select()
  }
}
function startRename(p: SyncProject): void {
  editingId.value = p.id
  draftName.value = p.name
}
async function commitRename(): Promise<void> {
  const id = editingId.value
  if (id == null) return
  const next = draftName.value.trim()
  editingId.value = null
  const current = sortedByName.value.find((p) => p.id === id)
  if (next && current && next !== current.name) {
    await projectsStore.rename(id, next)
  }
}
function cancelRename(): void {
  editingId.value = null
}

function onSetStatus(id: number, status: string): void {
  void projectsStore.setStatus(id, status)
}

// Delete.
const deleting = ref<{ id: number; name: string } | null>(null)
const deletePending = ref(false)
function askDelete(p: SyncProject): void {
  deleting.value = { id: p.id, name: p.name }
}
async function confirmDelete(): Promise<void> {
  if (!deleting.value) return
  deletePending.value = true
  try {
    await projectService.deleteProject(deleting.value.id)
    deleting.value = null
  } catch (e) {
    logger.error('Failed to delete project', e)
  } finally {
    deletePending.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-6 px-4 sm:px-6 py-6 max-w-6xl mx-auto w-full">
    <header class="flex flex-col gap-4">
      <div>
        <h1 class="text-2xl font-semibold text-primary">{{ $t('projects-list-heading') }}</h1>
        <p class="text-xs text-tertiary mt-1">{{ $t('projects-list-subheading') }}</p>
      </div>

      <div v-if="bootstrapped && sortedByName.length > 0" class="flex items-center gap-2">
        <div class="flex-1 max-w-xs">
          <DebouncedSearchInput v-model="search" />
        </div>
        <div class="w-40">
          <BaseDropdown
            :model-value="statusFilter"
            :options="statusFilterOptions"
            size="sm"
            @update:model-value="onStatusFilter"
          />
        </div>
      </div>
    </header>

    <!-- Loading skeleton -->
    <div
      v-if="isInitiallyLoading"
      class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4"
    >
      <div
        v-for="i in 6"
        :key="i"
        class="flex flex-col gap-3 bg-surface border border-subtle rounded-lg p-4 animate-pulse"
      >
        <div class="h-5 w-2/3 bg-surface-alt rounded"></div>
        <div class="h-3 w-1/3 bg-surface-alt rounded"></div>
      </div>
    </div>

    <!-- Empty workspace -->
    <div
      v-else-if="isEmpty"
      class="flex flex-col items-center justify-center text-center py-16 px-4"
    >
      <div class="w-12 h-12 rounded-xl bg-surface-alt border border-subtle flex items-center justify-center mb-4">
        <svg class="w-6 h-6 text-tertiary" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
        </svg>
      </div>
      <h2 class="text-base font-medium text-primary">{{ $t('projects-empty-title') }}</h2>
      <p class="text-sm text-secondary mt-1 max-w-sm">{{ $t('projects-empty-subtitle') }}</p>
      <Button variant="primary" class="mt-5" @click="createOpen = true">
        {{ $t('projects-empty-cta') }}
      </Button>
    </div>

    <!-- Filtered to nothing -->
    <div
      v-else-if="noMatches"
      class="flex flex-col items-center justify-center text-center py-16 text-sm text-secondary"
    >
      {{ $t('projects-list-no-results') }}
    </div>

    <!-- Project grid -->
    <div v-else class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
      <div
        v-for="project in filtered"
        :key="project.id"
        class="group flex flex-col gap-3 bg-surface border border-default hover:border-strong rounded-lg p-4 cursor-pointer transition-colors"
        @click="open(project.id)"
      >
        <div class="flex items-start justify-between gap-2">
          <input
            v-if="editingId === project.id"
            :ref="focusRename"
            v-model="draftName"
            type="text"
            class="flex-1 min-w-0 text-base font-medium text-primary bg-surface-alt border border-default rounded px-2 py-0.5 focus:outline-none focus:border-accent"
            :aria-label="$t('project-actions-rename')"
            @click.stop
            @keyup.enter="commitRename"
            @keyup.esc="cancelRename"
            @blur="commitRename"
          />
          <button
            v-else
            type="button"
            class="flex-1 min-w-0 text-left text-base font-medium text-primary truncate group-hover:text-accent transition-colors"
            @click.stop="open(project.id)"
          >
            {{ project.name }}
          </button>
          <ProjectActionsMenu
            class="shrink-0"
            :status="project.status"
            @click.stop
            @rename="startRename(project)"
            @set-status="(s) => onSetStatus(project.id, s)"
            @delete="askDelete(project)"
          />
        </div>

        <p v-if="project.description" class="text-sm text-secondary line-clamp-2">
          {{ project.description }}
        </p>
        <p v-else class="text-sm text-tertiary italic">{{ $t('projects-list-no-description') }}</p>

        <div class="flex items-center gap-3 mt-auto pt-1 text-xs text-tertiary">
          <span class="inline-flex items-center gap-1.5">
            <span class="w-1.5 h-1.5 rounded-full" :class="statusDot(project.status)" />
            {{ $t(`project-actions-status-${project.status}`) }}
          </span>
          <span>{{ $t('project-detail-ticket-count', { count: ticketCount(project.id) }) }}</span>
          <span class="ml-auto inline-flex items-center gap-1.5">
            <UserAvatar v-if="project.created_by" :uuid="project.created_by" size="xxs" />
            <span v-if="project.updated_at">{{ formatRelativeTime(project.updated_at) }}</span>
          </span>
        </div>
      </div>
    </div>

    <CreateProjectModal
      v-if="createOpen"
      @close="createOpen = false"
      @created="createOpen = false"
    />

    <ConfirmModal
      :show="!!deleting"
      variant="danger"
      :title="$t('project-delete-confirm-title')"
      :message="$t('project-delete-confirm-message', { name: deleting?.name ?? '' })"
      :confirm-label="$t('project-delete-confirm-button')"
      :loading="deletePending"
      @confirm="confirmDelete"
      @close="deleting = null"
    />
  </div>
</template>
