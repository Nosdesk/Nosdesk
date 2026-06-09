<script setup lang="ts">
/**
 * Projects list — sync-engine version. Renders from the sync runtime's
 * object pool (live via the SSE outbox). Each project is enriched, from
 * the same pool, with a ticket-status breakdown, team, and active
 * cycle, plus deep links to its Board/Gantt/Cycles views.
 *
 * Desktop uses the shared DataTable (the same component assets/users
 * use) so projects gets draggable / resizable / sortable column headers
 * with persisted layout for free; mobile keeps the enriched cards.
 * Rename/status/delete side effects are owned here.
 */
import { onMounted, computed, ref, type ComponentPublicInstance } from 'vue'
import { useRouter } from 'vue-router'
import { storeToRefs } from 'pinia'
import { useFluent } from 'fluent-vue'
import { subscribe } from '@/sync/lifecycle'
import { useSyncProjectsStore, type SyncProject } from '@/sync/stores/projects'
import { useProjectRollups } from '@/composables/useProjectRollups'
import { useActiveCycleSummaries } from '@/composables/useActiveCycleSummaries'
import { useDataTableColumns } from '@/composables/useDataTableColumns'
import { useProjectsDensity } from '@/composables/useTicketsDensity'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { projectStatusDot } from '@/utils/projectStatus'
import { formatCompactRelativeTime } from '@/utils/dateUtils'
import { logger } from '@/utils/logger'
import DataTable from '@/components/common/DataTable.vue'
import ListDensityToggle from '@/components/common/ListDensityToggle.vue'
import CreateProjectModal from '@/components/projectComponents/CreateProjectModal.vue'
import ProjectCard from '@/components/projectComponents/ProjectCard.vue'
import ProjectStatusBar from '@/components/projectComponents/ProjectStatusBar.vue'
import ProjectQuickNav from '@/components/projectComponents/ProjectQuickNav.vue'
import ProjectCycleGlance from '@/components/projectComponents/ProjectCycleGlance.vue'
import ProjectActionsMenu from '@/components/projectComponents/ProjectActionsMenu.vue'
import AvatarStack from '@/components/common/AvatarStack.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import ContextMenu from '@/components/common/ContextMenu.vue'
import type { MenuItem } from '@/components/common/ContextMenu.vue'
import type { Project } from '@/types/project'
import { buildProjectMenuItems } from '@/utils/projectMenuItems'

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

// Desktop table columns. Labels are translated; field ids stay stable
// so useDataTableColumns can persist order/width per id. Drag/resize/
// sort come from the shared DataTable.
const projectColumns = computed(() => [
  { field: 'name', label: t('projects-col-project'), width: 'minmax(220px, 1fr)', sortable: true, sortKey: 'name', minWidthPx: 160 },
  { field: 'progress', label: t('projects-col-progress'), width: 'minmax(150px, 220px)', sortable: true, sortKey: 'progress', minWidthPx: 120, maxWidthPx: 280 },
  { field: 'team', label: t('projects-col-team'), width: '110px', minWidthPx: 80, maxWidthPx: 180 },
  { field: 'cycle', label: t('projects-col-cycle'), width: 'minmax(170px, 240px)', minWidthPx: 140 },
  { field: 'links', label: '', width: '160px', minWidthPx: 120, maxWidthPx: 220 },
  { field: 'updated', label: t('projects-col-updated'), width: '90px', sortable: true, sortKey: 'updated', minWidthPx: 64, maxWidthPx: 140 },
  { field: 'actions', label: '', width: '52px', minWidthPx: 44, maxWidthPx: 72 },
])

const cols = useDataTableColumns({
  columns: projectColumns,
  storageNamespace: 'projects',
  getViewId: () => 'default',
  pinnedIds: ['name'],
})

const { density, setDensity, rowClass, cellPadding } = useProjectsDensity()

// Status filter (chips) + sort (driven by clicking the column headers).
// Finding a project by name is the global search's job, so there's no
// in-page search box.
const statusFilter = ref('all')
const statusFilterOptions = computed(() => [
  { value: 'all', label: t('projects-filter-status-all') },
  { value: 'active', label: t('project-actions-status-active') },
  { value: 'completed', label: t('project-actions-status-completed') },
  { value: 'archived', label: t('project-actions-status-archived') },
])
const statusCounts = computed<Record<string, number>>(() => {
  const c: Record<string, number> = { all: 0, active: 0, completed: 0, archived: 0 }
  for (const p of sortedByName.value) {
    c.all += 1
    if (p.status in c) c[p.status] += 1
  }
  return c
})

const sortField = ref('name')
const sortDir = ref<'asc' | 'desc'>('asc')

const filtered = computed(() =>
  statusFilter.value === 'all'
    ? sortedByName.value.slice()
    : sortedByName.value.filter((p) => p.status === statusFilter.value),
)

function progressRatio(p: SyncProject): number {
  const r = rollups.value.get(p.id)
  return r && r.total > 0 ? r.closed / r.total : 0
}

const displayed = computed(() => {
  const arr = [...filtered.value]
  const dir = sortDir.value === 'asc' ? 1 : -1
  arr.sort((a, b) => {
    let r = 0
    if (sortField.value === 'name') r = a.name.localeCompare(b.name)
    else if (sortField.value === 'updated')
      r = (a.updated_at ?? a.created_at).localeCompare(b.updated_at ?? b.created_at)
    else if (sortField.value === 'progress') r = progressRatio(a) - progressRatio(b)
    else if (sortField.value === 'tickets')
      r = (rollups.value.get(a.id)?.total ?? 0) - (rollups.value.get(b.id)?.total ?? 0)
    return r * dir
  })
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

// Table header click: DataTable already toggled the direction.
function onTableSort(field: string, dir: 'asc' | 'desc'): void {
  sortField.value = field
  sortDir.value = dir
}

// Inline rename (desktop table cell). DataTable owns the rows, so the
// edit state lives here; the name cell shows the input for editingId.
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
function commitRename(): void {
  const id = editingId.value
  if (id == null) return
  const name = draftName.value.trim()
  editingId.value = null
  void onRename(id, name)
}
function cancelRename(): void {
  editingId.value = null
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
function askDelete(p: SyncProject): void {
  deleting.value = { id: p.id, name: p.name }
}
async function confirmDelete(): Promise<void> {
  if (!deleting.value) return
  const { id } = deleting.value
  deleting.value = null
  try {
    await projectsStore.remove(id)
  } catch (e) {
    logger.error('Failed to delete project', e)
  }
}

function onProjectCreated(project: Project): void {
  createOpen.value = false
  projectsStore.ingestCreated(project)
  router.push({ name: 'project-detail', params: { id: String(project.id) } })
}

// Right-click context menu (desktop table + mobile cards).
const contextMenuProjectId = ref<number | null>(null)
const contextMenuPos = ref({ x: 0, y: 0 })
const showContextMenu = ref(false)

const contextMenuProject = computed(() => {
  const id = contextMenuProjectId.value
  if (id == null) return null
  return sortedByName.value.find((p) => p.id === id) ?? null
})

const projectContextMenuItems = computed<MenuItem[]>(() => {
  const project = contextMenuProject.value
  if (!project) return []
  return buildProjectMenuItems(project.status, t, { forContextMenu: true })
})

function handleProjectContextMenu(project: SyncProject, event: MouseEvent): void {
  contextMenuProjectId.value = project.id
  contextMenuPos.value = { x: event.clientX, y: event.clientY }
  showContextMenu.value = true
}

function handleProjectContextMenuSelect(actionId: string): void {
  const project = contextMenuProject.value
  if (!project) return

  switch (actionId) {
    case 'open':
      open(project.id)
      break
    case 'rename':
      startRename(project)
      break
    case 'delete':
      askDelete(project)
      break
    default:
      if (actionId.startsWith('status:')) {
        onSetStatus(project.id, actionId.slice('status:'.length))
      }
  }
}
</script>

<template>
  <div class="h-full flex flex-col">
    <!-- Status filter chips. One-click navigation by lifecycle, with a
         live count per status. Sorting is done by clicking the table
         column headers; finding a project by name is the global
         search's job. -->
    <div
      v-if="bootstrapped && sortedByName.length > 0"
      class="shrink-0 flex items-center gap-1 px-4 sm:px-6 py-2.5 border-b border-subtle"
    >
      <button
        v-for="option in statusFilterOptions"
        :key="option.value"
        type="button"
        class="inline-flex items-center gap-1.5 h-7 px-2.5 rounded-md text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        :class="statusFilter === option.value
          ? 'bg-accent/15 text-accent'
          : 'text-secondary hover:bg-surface-hover'"
        @click="statusFilter = option.value"
      >
        {{ option.label }}
        <span
          class="text-xs tabular-nums"
          :class="statusFilter === option.value ? 'text-accent/70' : 'text-tertiary'"
        >{{ statusCounts[option.value] ?? 0 }}</span>
      </button>

      <div class="flex-1 min-w-2" />

      <!-- Row density. Desktop table only (lg+). -->
      <ListDensityToggle
        class="hidden lg:inline-flex"
        :density="density"
        @set-density="setDensity"
      />
    </div>

    <!-- Loading skeleton -->
    <div
      v-if="isInitiallyLoading"
      class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4 p-4 sm:p-6"
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
    <EmptyState
      v-else-if="isEmpty"
      class="flex-1"
      icon="folder"
      :title="$t('projects-empty-title')"
      :description="$t('projects-empty-subtitle')"
      :action-label="$t('projects-empty-cta')"
      @action="createOpen = true"
    />

    <!-- Filtered to nothing -->
    <div
      v-else-if="noMatches"
      class="flex-1 flex flex-col items-center justify-center text-center py-16 text-sm text-secondary"
    >
      {{ $t('projects-list-no-results') }}
    </div>

    <!-- Scroll area: full-bleed table on desktop, padded cards below -->
    <div v-else class="flex-1 min-h-0 overflow-auto">
      <!-- Desktop: shared DataTable (draggable / resizable / sortable) -->
      <div class="hidden lg:block">
        <DataTable
          :columns="cols.visible.value"
          :data="displayed"
          :selected-items="[]"
          :selectable="false"
          :sort-field="sortField"
          :sort-direction="sortDir"
          :row-class="rowClass"
          :cell-padding="cellPadding"
          :column-reorder="cols.reorderBundle"
          :column-resize="cols.resizeBundle"
          @update:sort="onTableSort"
          @row-click="(p: SyncProject) => open(p.id)"
          @row-contextmenu="handleProjectContextMenu"
        >
          <template #cell-name="{ item }">
            <div class="flex items-center gap-2 min-w-0 w-full">
              <span
                class="block w-1.5 h-1.5 rounded-full shrink-0"
                :class="projectStatusDot(item.status)"
                :title="$t(`project-actions-status-${item.status}`)"
              />
              <input
                v-if="editingId === item.id"
                :ref="focusRename"
                v-model="draftName"
                type="text"
                class="w-full text-sm font-medium text-primary bg-surface-alt border border-default rounded px-2 py-0.5 focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
                :aria-label="$t('project-actions-rename')"
                @click.stop
                @keyup.enter="commitRename"
                @keyup.esc="cancelRename"
                @blur="commitRename"
              />
              <button
                v-else
                type="button"
                class="block max-w-full truncate text-left text-sm font-medium text-primary rounded transition-colors hover:text-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                @click.stop="open(item.id)"
              >
                {{ item.name }}
              </button>
            </div>
          </template>

          <template #cell-progress="{ item }">
            <div class="flex items-center gap-2 w-full">
              <ProjectStatusBar
                class="flex-1"
                :open="rollupOf(item.id)?.open ?? 0"
                :in-progress="rollupOf(item.id)?.inProgress ?? 0"
                :closed="rollupOf(item.id)?.closed ?? 0"
                :total="rollupOf(item.id)?.total ?? 0"
              />
              <span class="shrink-0 text-xs tabular-nums text-tertiary">
                {{ rollupOf(item.id)?.closed ?? 0 }}/{{ rollupOf(item.id)?.total ?? 0 }}
              </span>
            </div>
          </template>

          <template #cell-team="{ item }">
            <AvatarStack
              v-if="rollupOf(item.id)?.assignees.length"
              :uuids="rollupOf(item.id)!.assignees"
              :max="3"
              size="xs"
            />
          </template>

          <template #cell-cycle="{ item }">
            <ProjectCycleGlance v-if="activeCycleOf(item.id)" :summary="activeCycleOf(item.id)" compact />
            <span v-else class="text-xs text-tertiary">{{ $t('projects-no-active-cycle') }}</span>
          </template>

          <template #cell-links="{ item }">
            <ProjectQuickNav :project-id="item.id" />
          </template>

          <template #cell-updated="{ item }">
            <span v-if="item.updated_at" class="text-[11px] text-tertiary tabular-nums">
              {{ formatCompactRelativeTime(item.updated_at) }}
            </span>
          </template>

          <template #cell-actions="{ item }">
            <div @click.stop>
              <ProjectActionsMenu
                :status="item.status"
                @rename="startRename(item)"
                @set-status="(s: string) => onSetStatus(item.id, s)"
                @delete="askDelete(item)"
              />
            </div>
          </template>
        </DataTable>
      </div>

      <!-- Tablet / mobile: enriched cards -->
      <div class="lg:hidden grid grid-cols-1 sm:grid-cols-2 gap-4 p-4 sm:p-6">
        <ProjectCard
          v-for="project in displayed"
          :key="project.id"
          :project="project"
          :rollup="rollupOf(project.id)"
          :cycle="activeCycleOf(project.id)"
          @open="open(project.id)"
          @contextmenu="handleProjectContextMenu(project, $event)"
          @rename="(name) => onRename(project.id, name)"
          @set-status="(s) => onSetStatus(project.id, s)"
          @delete="askDelete(project)"
        />
      </div>
    </div>

    <CreateProjectModal
      v-if="createOpen"
      @close="createOpen = false"
      @created="onProjectCreated"
    />

    <ConfirmModal
      :show="!!deleting"
      variant="danger"
      :title="$t('project-delete-confirm-title')"
      :message="$t('project-delete-confirm-message', { name: deleting?.name ?? '' })"
      :confirm-label="$t('project-delete-confirm-button')"
      @confirm="confirmDelete"
      @close="deleting = null"
    />

    <ContextMenu
      :open="showContextMenu"
      :items="projectContextMenuItems"
      :x="contextMenuPos.x"
      :y="contextMenuPos.y"
      @select="handleProjectContextMenuSelect"
      @close="showContextMenu = false"
    />
  </div>
</template>
