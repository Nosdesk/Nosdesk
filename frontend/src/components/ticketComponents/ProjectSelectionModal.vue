<!-- ProjectSelectionModal.vue -->
<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import Modal from '@/components/Modal.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import type { Project, ProjectStatus } from '@/types/project'
import { useSyncProjectsStore, type SyncProject } from '@/sync/stores/projects'
import { useAggregate } from '@/sync/composables'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{
  show: boolean;
  existingProjectIds?: number[];
}>()

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'select-project', project: Project): void;
}>()

const searchQuery = ref('')

// Projects come straight from the sync pool — bounded, already
// hydrated — so the picker opens instantly with no fetch or spinner.
const projectsStore = useSyncProjectsStore()

// Ticket counts derived from the project_ticket aggregate so the
// column stays accurate without a REST round-trip.
const projectTickets = useAggregate<{ project_id: number; ticket_id: number }>('project_ticket')
const ticketCountById = computed(() => {
  const counts = new Map<number, number>()
  for (const row of projectTickets.value) {
    counts.set(row.project_id, (counts.get(row.project_id) ?? 0) + 1)
  }
  return counts
})

function toProject(p: SyncProject): Project {
  return {
    id: p.id,
    name: p.name,
    description: p.description,
    status: p.status as ProjectStatus,
    created_at: p.created_at,
    updated_at: p.updated_at ?? p.created_at,
    ticket_count: ticketCountById.value.get(p.id) ?? 0,
  }
}

// Reset the search each time the modal opens.
watch(
  () => props.show,
  (isVisible) => {
    if (isVisible) searchQuery.value = ''
  },
)

const filteredProjects = computed<Project[]>(() => {
  const query = searchQuery.value.trim().toLowerCase()
  return projectsStore.sortedByName
    .filter(
      (p: SyncProject) =>
        !query ||
        p.name.toLowerCase().includes(query) ||
        (p.description?.toLowerCase().includes(query) ?? false),
    )
    .map(toProject)
})

const getStatusClass = (status: string) => {
  switch (status) {
    case 'active':
      return 'bg-status-success/30 text-status-success border-status-success/30'
    case 'completed':
      return 'bg-accent/15 text-accent border-accent/30'
    case 'archived':
      return 'bg-surface-alt/30 text-secondary border-subtle/30'
    default:
      return 'bg-surface-alt/30 text-tertiary border-subtle/30'
  }
}

const isAdded = (id: number) => props.existingProjectIds?.includes(id) ?? false

const selectProject = (project: Project) => {
  if (isAdded(project.id)) return
  emit('select-project', project)
}
</script>

<template>
  <Modal
    :show="show"
    :title="t('project-modal-title')"
    @close="emit('close')"
    size="lg"
  >
    <div class="flex flex-col gap-4">
      <!-- Search -->
      <div class="relative">
        <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
          <svg class="h-5 w-5 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
        </div>
        <input
          v-model="searchQuery"
          type="text"
          :placeholder="t('project-modal-search-placeholder')"
          class="w-full pl-10 pr-4 py-2.5 rounded-lg border border-default bg-surface-alt text-primary placeholder-tertiary transition-colors duration-200 hover:border-strong focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
        />
      </div>

      <!-- No results for a search -->
      <EmptyState
        v-if="filteredProjects.length === 0 && searchQuery"
        icon="search"
        :title="$t('empty-project-search-title')"
        :description="$t('empty-project-search-description')"
        variant="compact"
      />

      <!-- No projects at all -->
      <EmptyState
        v-else-if="filteredProjects.length === 0"
        icon="folder"
        :title="$t('empty-project-available-title')"
        :description="$t('empty-project-available-description')"
        variant="compact"
      />

      <!-- Projects list -->
      <div
        v-else
        class="max-h-[500px] overflow-y-auto"
      >
        <div class="bg-surface-alt rounded-lg border border-default overflow-hidden">
          <!-- Table header. Hidden below md: rows render as stacked
               cards there, so a column header would be meaningless. -->
          <div class="hidden md:block bg-surface-alt px-4 py-3 border-b border-default sticky top-0 z-10">
            <div class="grid grid-cols-12 gap-3 text-xs font-medium text-secondary uppercase tracking-wide">
              <div class="col-span-4">{{ t('project-modal-col-name') }}</div>
              <div class="col-span-4">{{ t('project-modal-col-description') }}</div>
              <div class="col-span-2">{{ t('project-modal-col-status') }}</div>
              <div class="col-span-1">{{ t('project-modal-col-tickets') }}</div>
              <div class="col-span-1 text-right">{{ t('project-modal-col-action') }}</div>
            </div>
          </div>

          <!-- Project rows -->
          <div class="divide-y divide-subtle">
            <div
              v-for="project in filteredProjects"
              :key="project.id"
              class="group relative hover:bg-surface-hover transition-colors duration-150 cursor-pointer"
              :class="{ 'bg-accent/10 border-l-4 border-accent': isAdded(project.id) }"
              @click="selectProject(project)"
            >
              <!-- Already added indicator -->
              <div v-if="isAdded(project.id)" class="absolute -top-1 right-2 z-10">
                <div class="bg-accent text-on-accent text-xs px-2 py-0.5 rounded-b-md shadow-sm">
                  {{ t('project-modal-already-added') }}
                </div>
              </div>

              <div class="px-4 py-3">
                <!-- Stacked card below md (scan/tap one record at a time);
                     the 12-col table layout returns at md+. -->
                <div class="flex flex-col gap-1.5 md:grid md:grid-cols-12 md:gap-3 md:items-center">
                  <!-- Project Name -->
                  <div class="col-span-4 min-w-0">
                    <div class="font-medium text-primary truncate text-sm" :title="project.name">
                      {{ project.name }}
                    </div>
                  </div>

                  <!-- Description -->
                  <div class="col-span-4 min-w-0">
                    <div v-if="project.description" class="text-sm text-secondary truncate" :title="project.description">
                      {{ project.description }}
                    </div>
                    <div v-else class="text-sm text-tertiary italic">
                      {{ t('project-modal-no-description') }}
                    </div>
                  </div>

                  <!-- Status -->
                  <div class="col-span-2 min-w-0">
                    <span
                      :class="getStatusClass(project.status)"
                      class="text-xs px-2 py-1 rounded-full border capitalize"
                    >
                      {{ project.status }}
                    </span>
                  </div>

                  <!-- Ticket Count. The bare number needs a label on
                       mobile where the column header is hidden. -->
                  <div class="col-span-1 min-w-0">
                    <span class="text-sm text-secondary font-mono">{{ project.ticket_count }}</span>
                    <span class="md:hidden text-xs text-tertiary">
                      {{ ' ' }}{{ t('project-modal-col-tickets').toLowerCase() }}
                    </span>
                  </div>

                  <!-- Action -->
                  <div class="col-span-1 text-right">
                    <button
                      v-if="!isAdded(project.id)"
                      class="text-accent hover:text-accent text-xs font-medium px-2 py-1 rounded hover:bg-accent/10 transition-colors"
                    >
                      {{ t('project-modal-select') }}
                    </button>
                    <span
                      v-else
                      class="text-tertiary text-xs font-medium px-2 py-1"
                    >
                      {{ t('project-modal-added') }}
                    </span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Footer -->
    <div class="mt-6 flex justify-between items-center pt-4">
      <span class="text-sm text-tertiary">
        {{ t('project-modal-count', { count: filteredProjects.length }) }}
      </span>

      <button
        type="button"
        class="px-4 py-2 text-sm text-secondary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
        @click="emit('close')"
      >
        {{ t('project-modal-cancel') }}
      </button>
    </div>
  </Modal>
</template>
