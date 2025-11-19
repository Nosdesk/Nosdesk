<!-- ProjectSelectionModal.vue -->
<script setup lang="ts">
import { formatDate as formatDateUtil, formatDateTime } from '@/utils/dateUtils';
import { ref, computed, watch, onMounted, nextTick } from 'vue'
import Modal from '@/components/Modal.vue'
import type { Project } from '@/types/project'
import { projectService } from '@/services/projectService'

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
const performSearch = (query: string) => {
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
      return 'bg-green-900/30 text-green-400 border-green-700/30'
    case 'completed':
      return 'bg-blue-900/30 text-blue-400 border-blue-700/30'
    case 'archived':
      return 'bg-gray-900/30 text-gray-400 border-gray-700/30'
    default:
      return 'bg-slate-700/30 text-slate-400 border-slate-600/30'
  }
}

const selectProject = (project: Project) => {
  // Don't select if already added
  if (props.existingProjectIds?.includes(project.id)) {
    return;
  }
  emit('select-project', project)
}

// Format date for display
const formatDate = (dateString: string): string => {
  try {
    const date = new Date(dateString);
    const now = new Date();
    const diffTime = now.getTime() - date.getTime();
    const diffDays = Math.floor(diffTime / (1000 * 60 * 60 * 24));

    if (diffDays === 0) {
      return 'Today';
    } else if (diffDays === 1) {
      return 'Yesterday';
    } else if (diffDays < 7) {
      return `${diffDays}d ago`;
    } else {
      return formatDateUtil(dateString, 'MMM d, yyyy');
    }
  } catch (e) {
    return 'Unknown';
  }
};
</script>

<template>
  <Modal
    :show="show"
    title="Add to Project"
    @close="emit('close')"
    size="lg"
  >
    <div class="flex flex-col gap-4">
      <!-- Search -->
      <div class="relative">
        <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
          <svg class="h-5 w-5 text-slate-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
        </div>
        <input
          v-model="searchQuery"
          type="text"
          placeholder="Search projects by name or description..."
          class="w-full pl-10 pr-4 py-3 rounded-lg border border-default bg-surface text-primary placeholder-tertiary focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 transition-colors"
        />
        <div v-if="isLoading && searchQuery" class="absolute inset-y-0 right-0 pr-3 flex items-center">
          <svg class="w-5 h-5 animate-spin text-slate-400" fill="none" viewBox="0 0 24 24">
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
          <span>Loading projects...</span>
        </div>
      </div>

      <!-- Error state -->
      <div v-else-if="error" class="text-center py-8">
        <div class="bg-red-900/20 border border-red-700/30 rounded-lg p-4">
          <p class="text-red-400 flex items-center justify-center gap-2">
            <svg class="w-5 h-5 flex-shrink-0" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
            </svg>
            {{ error }}
          </p>
          <button 
            @click="fetchProjects()"
            class="mt-3 px-4 py-2 bg-red-600 text-white rounded-md hover:bg-red-700 transition-colors text-sm"
          >
            Try Again
          </button>
        </div>
      </div>

      <!-- No results -->
      <div v-else-if="!isLoading && filteredProjects.length === 0 && searchQuery" class="text-center py-8 text-tertiary">
        <div class="inline-flex flex-col items-center gap-3">
          <svg class="w-12 h-12 text-slate-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
          </svg>
          <div class="text-center">
            <p class="text-lg font-medium text-secondary">No projects found</p>
            <p class="text-sm">Try adjusting your search criteria</p>
          </div>
        </div>
      </div>

      <!-- No projects available -->
      <div v-else-if="!isLoading && filteredProjects.length === 0 && !searchQuery" class="text-center py-8 text-tertiary">
        <div class="inline-flex flex-col items-center gap-3">
          <svg class="w-12 h-12 text-slate-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
          </svg>
          <div class="text-center">
            <p class="text-lg font-medium text-secondary">No projects available</p>
            <p class="text-sm">Create a project to get started</p>
          </div>
        </div>
      </div>

      <!-- Projects list -->
      <div 
        v-else-if="filteredProjects.length > 0"
        ref="scrollContainer"
        class="max-h-[500px] overflow-y-auto"
      >
        <div class="bg-surface-alt rounded-lg border border-default overflow-hidden">
          <!-- Table header -->
          <div class="bg-surface px-4 py-3 border-b border-default sticky top-0 z-10">
            <div class="grid grid-cols-12 gap-3 text-xs font-medium text-secondary uppercase tracking-wide">
              <div class="col-span-4">Project Name</div>
              <div class="col-span-4">Description</div>
              <div class="col-span-2">Status</div>
              <div class="col-span-1">Tickets</div>
              <div class="col-span-1 text-right">Action</div>
            </div>
          </div>
          
          <!-- Project rows -->
          <div class="divide-y divide-subtle">
            <div
              v-for="project in filteredProjects"
              :key="project.id"
              class="group relative hover:bg-surface-hover transition-colors duration-150 cursor-pointer"
              :class="{ 'bg-blue-900/20 border-l-4 border-blue-500': existingProjectIds?.includes(project.id) }"
              @click="selectProject(project)"
            >
              <!-- Already added indicator -->
              <div v-if="existingProjectIds?.includes(project.id)" class="absolute -top-1 right-2 z-10">
                <div class="bg-blue-500 text-white text-xs px-2 py-0.5 rounded-b-md shadow-sm">
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
                    <span class="text-sm text-secondary font-mono">{{ project.ticketCount }}</span>
                  </div>

                  <!-- Action Button -->
                  <div class="col-span-1 text-right">
                    <button
                      v-if="!existingProjectIds?.includes(project.id)"
                      class="text-blue-400 hover:text-blue-300 text-xs font-medium px-2 py-1 rounded hover:bg-blue-900/20 transition-colors"
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
    <div class="mt-6 flex justify-between items-center pt-4 border-t border-default">
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