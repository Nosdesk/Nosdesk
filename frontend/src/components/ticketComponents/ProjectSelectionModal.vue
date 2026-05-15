<!-- ProjectSelectionModal.vue -->
<script setup lang="ts">
import { ref, computed, watch, onMounted, nextTick } from 'vue'
import { useFluent } from 'fluent-vue'
import Modal from '@/components/Modal.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import type { Project } from '@/types/project'
import { projectService } from '@/services/projectService'

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

// State management
const projects = ref<Project[]>([])
const isLoading = ref(false)
const error = ref<string | null>(null)
const searchQuery = ref('')

// Search debouncing
let searchTimeout: ReturnType<typeof setTimeout> | null = null;
const searchDebounceMs = 300;

// Scroll container reference
const scrollContainer = ref<HTMLElement | null>(null);

// Fetch projects when the modal is shown
watch(() => props.show, async (isVisible) => {
  if (isVisible) {
    // Reset state
    searchQuery.value = '';
    error.value = null;
    
    // Load initial data
    nextTick(() => {
      fetchProjects();
    });
  } else {
    // Clear search timeout when modal closes
    if (searchTimeout) {
      clearTimeout(searchTimeout);
      searchTimeout = null;
    }
  }
})

// Fetch projects on component mount
onMounted(async () => {
  if (props.show) {
    await fetchProjects()
  }
})

const fetchProjects = async () => {
  isLoading.value = true
  error.value = null
  
  try {
    projects.value = await projectService.getProjects()
  } catch (err) {
    console.error('Failed to fetch projects:', err)
    error.value = 'Failed to load projects. Please try again later.'
    projects.value = []
  } finally {
    isLoading.value = false
  }
}

// Debounced search function
const performSearch = (_query: string) => {
  if (searchTimeout) {
    clearTimeout(searchTimeout);
  }
  
  searchTimeout = setTimeout(() => {
    // Search is performed on already loaded projects
    // No need to reload from API for client-side filtering
  }, searchDebounceMs);
};

const filteredProjects = computed(() => {
  const query = searchQuery.value.toLowerCase()
  return projects.value.filter(project => 
    project.name.toLowerCase().includes(query) ||
    project.description?.toLowerCase().includes(query)
  )
})

// Watch for search query changes
watch(searchQuery, (newQuery) => {
  performSearch(newQuery);
});

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

const selectProject = (project: Project) => {
  // Don't select if already added
  if (props.existingProjectIds?.includes(project.id)) {
    return;
  }
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
        <div v-if="isLoading && searchQuery" class="absolute inset-y-0 right-0 pr-3 flex items-center">
          <svg class="w-5 h-5 animate-spin text-tertiary" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
        </div>
      </div>

      <!-- Loading state (initial load) -->
      <div v-if="isLoading && projects.length === 0" class="text-center py-8 text-tertiary">
        <div class="inline-flex items-center gap-3">
          <svg class="w-5 h-5 animate-spin" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          <span>{{ t('common-loading-projects') }}</span>
        </div>
      </div>

      <!-- Error state -->
      <div v-else-if="error" class="text-center py-8">
        <div class="bg-status-error/20 border border-status-error/30 rounded-lg p-4">
          <p class="text-status-error flex items-center justify-center gap-2">
            <svg class="w-5 h-5 flex-shrink-0" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
            </svg>
            {{ error }}
          </p>
          <button
            @click="fetchProjects()"
            class="mt-3 px-4 py-2 bg-status-error/80 text-white rounded-md hover:bg-status-error transition-colors text-sm"
          >
            Try Again
          </button>
        </div>
      </div>

      <!-- No results -->
      <EmptyState
        v-else-if="!isLoading && filteredProjects.length === 0 && searchQuery"
        icon="search"
        :title="$t('empty-project-search-title')"
        :description="$t('empty-project-search-description')"
        variant="compact"
      />

      <!-- No projects available -->
      <EmptyState
        v-else-if="!isLoading && filteredProjects.length === 0 && !searchQuery"
        icon="folder"
        :title="$t('empty-project-available-title')"
        :description="$t('empty-project-available-description')"
        variant="compact"
      />

      <!-- Projects list -->
      <div 
        v-else-if="filteredProjects.length > 0"
        ref="scrollContainer"
        class="max-h-[500px] overflow-y-auto"
      >
        <div class="bg-surface-alt rounded-lg border border-default overflow-hidden">
          <!-- Table header -->
          <div class="bg-surface-alt px-4 py-3 border-b border-default sticky top-0 z-10">
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
              :class="{ 'bg-accent/10 border-l-4 border-accent': existingProjectIds?.includes(project.id) }"
              @click="selectProject(project)"
            >
              <!-- Already added indicator -->
              <div v-if="existingProjectIds?.includes(project.id)" class="absolute -top-1 right-2 z-10">
                <div class="bg-accent text-white text-xs px-2 py-0.5 rounded-b-md shadow-sm">
                  Already Added
                </div>
              </div>

              <div class="px-4 py-3">
                <div class="grid grid-cols-12 gap-3 items-center">
                  <!-- Project Name -->
                  <div class="col-span-4 min-w-0">
                    <div class="flex flex-col gap-1">
                      <div class="font-medium text-primary truncate text-sm" :title="project.name">
                        {{ project.name }}
                      </div>
                    </div>
                  </div>

                  <!-- Description -->
                  <div class="col-span-4 min-w-0">
                    <div v-if="project.description" class="text-sm text-secondary truncate" :title="project.description">
                      {{ project.description }}
                    </div>
                    <div v-else class="text-sm text-tertiary italic">
                      No description
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

                  <!-- Ticket Count -->
                  <div class="col-span-1 min-w-0">
                    <span class="text-sm text-secondary font-mono">{{ project.ticket_count }}</span>
                  </div>

                  <!-- Action Button -->
                  <div class="col-span-1 text-right">
                    <button
                      v-if="!existingProjectIds?.includes(project.id)"
                      class="text-accent hover:text-accent text-xs font-medium px-2 py-1 rounded hover:bg-accent/10 transition-colors"
                    >
                      Select
                    </button>
                    <span
                      v-else
                      class="text-tertiary text-xs font-medium px-2 py-1"
                    >
                      Added
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
      <div class="flex items-center gap-2 text-sm text-tertiary">
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
        </svg>
        <span>
          {{ filteredProjects.length }} project{{ filteredProjects.length !== 1 ? 's' : '' }} available
        </span>
      </div>

      <button
        type="button"
        class="px-4 py-2 text-sm text-secondary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
        @click="emit('close')"
      >
        Cancel
      </button>
    </div>
  </Modal>
</template> 