<script setup lang="ts">
/**
 * Projects list — sync-engine version. Renders from the sync runtime's
 * object pool (live via the SSE outbox). Each project is enriched, from
 * the same pool, with a ticket-status breakdown, team, and active
 * cycle, plus deep links to its Board/Gantt/Cycles views. Desktop gets
 * a dense row layout; mobile gets enriched cards. Both share the same
 * data and sub-components; only the container differs.
 *
 * Rename/status/delete orchestration lives here; the row/card emit up
 * (mirroring ProjectActionsMenu), so there's one owner of the side
 * effects across both layouts.
 */
import { onMounted, computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { storeToRefs } from 'pinia'
import { useFluent } from 'fluent-vue'
import { subscribe } from '@/sync/lifecycle'
import { useSyncProjectsStore, type SyncProject } from '@/sync/stores/projects'
import { useProjectRollups } from '@/composables/useProjectRollups'
import { useActiveCycleSummaries } from '@/composables/useActiveCycleSummaries'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { projectService } from '@/services/projectService'
import { logger } from '@/utils/logger'
import CreateProjectModal from '@/components/projectComponents/CreateProjectModal.vue'
import ProjectRow from '@/components/projectComponents/ProjectRow.vue'
import ProjectCard from '@/components/projectComponents/ProjectCard.vue'
import Button from '@/components/common/Button.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'

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

// Enrichment, all derived from the pool (no extra requests beyond the
// single active-cycle list fetch).
const rollups = useProjectRollups()
const rollupOf = (id: number) => rollups.value.get(id) ?? null
const { byProject: activeCycles } = useActiveCycleSummaries()
const activeCycleOf = (id: number) => activeCycles.value.get(id) ?? null

// Search + status filter + sort.
const search = ref('')
const statusFilter = ref('all')
const statusFilterOptions = computed(() => [
  { value: 'all', label: t('projects-filter-status-all') },
  { value: 'active', label: t('project-actions-status-active') },
  { value: 'completed', label: t('project-actions-status-completed') },
  { value: 'archived', label: t('project-actions-status-archived') },
])

type SortKey = 'name' | 'recent' | 'progress' | 'tickets'
const sortKey = ref<SortKey>('name')
const sortOptions = computed(() => [
  { value: 'name', label: t('projects-sort-name') },
  { value: 'recent', label: t('projects-sort-recent') },
  { value: 'progress', label: t('projects-sort-progress') },
  { value: 'tickets', label: t('projects-sort-tickets') },
])

const filtered = computed(() => {
  const q = search.value.trim().toLowerCase()
  return sortedByName.value.filter(
    (p) =>
      (statusFilter.value === 'all' || p.status === statusFilter.value) &&
      (q === '' || p.name.toLowerCase().includes(q)),
  )
})

function progressRatio(p: SyncProject): number {
  const r = rollups.value.get(p.id)
  return r && r.total > 0 ? r.closed / r.total : 0
}

// `sortedByName` already gives the name order, so 'name' is a no-op;
// the others re-sort a copy.
const displayed = computed(() => {
  const arr = [...filtered.value]
  if (sortKey.value === 'recent') {
    arr.sort((a, b) =>
      (b.updated_at ?? b.created_at).localeCompare(a.updated_at ?? a.created_at),
    )
  } else if (sortKey.value === 'tickets') {
    arr.sort((a, b) => (rollups.value.get(b.id)?.total ?? 0) - (rollups.value.get(a.id)?.total ?? 0))
  } else if (sortKey.value === 'progress') {
    arr.sort((a, b) => progressRatio(b) - progressRatio(a))
  }
  return arr
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

function onSort(value: string | string[]): void {
  sortKey.value = (Array.isArray(value) ? value[0] : value) as SortKey
}

async function onRename(id: number, name: string): Promise<void> {
  const current = sortedByName.value.find((p) => p.id === id)
  if (name && current && name !== current.name) {
    await projectsStore.rename(id, name)
  }
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

      <div
        v-if="bootstrapped && sortedByName.length > 0"
        class="flex flex-wrap items-center gap-2"
      >
        <div class="flex-1 min-w-48 max-w-xs">
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
        <label class="flex items-center gap-2 text-xs text-secondary">
          <span class="shrink-0">{{ $t('projects-sort-label') }}</span>
          <div class="w-40">
            <BaseDropdown
              :model-value="sortKey"
              :options="sortOptions"
              size="sm"
              @update:model-value="onSort"
            />
          </div>
        </label>
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

    <template v-else>
      <!-- Desktop: dense rows -->
      <div class="hidden md:block border border-subtle rounded-lg overflow-hidden">
        <div
          class="flex items-center gap-3 px-3 py-2 border-b border-subtle bg-surface text-[11px] uppercase tracking-wide font-semibold text-tertiary"
        >
          <div class="flex items-center gap-2 min-w-0 flex-1">
            <span class="w-1.5 h-1.5 shrink-0"></span>{{ $t('projects-col-project') }}
          </div>
          <div class="w-44 shrink-0">{{ $t('projects-col-progress') }}</div>
          <div class="w-20 shrink-0">{{ $t('projects-col-team') }}</div>
          <div class="w-48 shrink-0">{{ $t('projects-col-cycle') }}</div>
          <div class="w-40 shrink-0"></div>
          <div class="w-12 shrink-0 text-right">{{ $t('projects-col-updated') }}</div>
          <div class="w-8 shrink-0"></div>
        </div>
        <div class="divide-y divide-subtle">
          <ProjectRow
            v-for="project in displayed"
            :key="project.id"
            :project="project"
            :rollup="rollupOf(project.id)"
            :cycle="activeCycleOf(project.id)"
            @open="open(project.id)"
            @rename="(name) => onRename(project.id, name)"
            @set-status="(s) => onSetStatus(project.id, s)"
            @delete="askDelete(project)"
          />
        </div>
      </div>

      <!-- Mobile: enriched cards -->
      <div class="md:hidden grid grid-cols-1 sm:grid-cols-2 gap-4">
        <ProjectCard
          v-for="project in displayed"
          :key="project.id"
          :project="project"
          :rollup="rollupOf(project.id)"
          :cycle="activeCycleOf(project.id)"
          @open="open(project.id)"
          @rename="(name) => onRename(project.id, name)"
          @set-status="(s) => onSetStatus(project.id, s)"
          @delete="askDelete(project)"
        />
      </div>
    </template>

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
