<!-- ProjectsView.vue -->
<script setup lang="ts">
import { ref, computed, onMounted, onActivated, onDeactivated, onUnmounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import type { Project, ProjectStatus } from '@/types/project'
import Modal from '@/components/Modal.vue'
import ProjectForm from '@/components/projectComponents/ProjectForm.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import { projectService } from '@/services/projectService'
import { formatRelativeTime, formatCompactRelativeTime } from '@/utils/dateUtils'
import { useStaggeredList } from '@/composables/useStaggeredList'
import { useMobileSearch } from '@/composables/useMobileSearch'
import EmptyState from '@/components/common/EmptyState.vue'
import ErrorBanner from '@/components/common/ErrorBanner.vue'

const projects = ref<Project[]>([])
const router = useRouter()
const showCreateModal = ref(false)
const showEditModal = ref(false)
const isLoading = ref(true)
const error = ref<string | null>(null)
const selectedProject = ref<Project | null>(null)
const searchQuery = ref('')
const statusFilter = ref<ProjectStatus | 'all'>('all')

// Status options for filter
const statusOptions: { value: ProjectStatus | 'all', label: string }[] = [
  { value: 'all', label: 'All Status' },
  { value: 'active', label: 'Active' },
  { value: 'completed', label: 'Completed' },
  { value: 'archived', label: 'Archived' },
]

// Filtered projects
const filteredProjects = computed(() => {
  let result = projects.value

  // Filter by status
  if (statusFilter.value !== 'all') {
    result = result.filter(p => p.status === statusFilter.value)
  }

  // Filter by search query
  if (searchQuery.value.trim()) {
    const query = searchQuery.value.toLowerCase()
    result = result.filter(p =>
      p.name.toLowerCase().includes(query) ||
      (p.description?.toLowerCase().includes(query))
    )
  }

  return result
})

// Staggered fade-in animation
const { getStyle } = useStaggeredList()

// Status badge classes using semantic tokens
const statusClasses: Record<ProjectStatus, string> = {
  active: 'bg-status-success-muted text-status-success border-status-success/30',
  completed: 'bg-status-info-muted text-status-info border-status-info/30',
  archived: 'bg-surface-alt text-secondary border-default',
}

const getStatusClass = (status: ProjectStatus) => statusClasses[status] || statusClasses.archived

// Format status for display
const formatStatus = (status: string) => status.charAt(0).toUpperCase() + status.slice(1)

// Fetch projects on component mount
onMounted(async () => {
  try {
    isLoading.value = true
    projects.value = await projectService.getProjects()
  } catch (err) {
    console.error('Failed to fetch projects:', err)
    error.value = 'Failed to load projects. Please try again later.'
  } finally {
    isLoading.value = false
  }
})

const openProject = (projectId: number) => {
  router.push(`/projects/${projectId}`)
}

const handleEditProject = async (projectData: Omit<Project, 'id' | 'ticket_count'> & { id?: number }) => {
  if (!selectedProject.value?.id) return

  try {
    isLoading.value = true
    error.value = null

    const updatedProject = await projectService.updateProject(
      selectedProject.value.id,
      projectData
    )

    // Update the project in the local array
    const index = projects.value.findIndex(p => p.id === selectedProject.value?.id)
    if (index !== -1) {
      projects.value[index] = updatedProject
    }

    showEditModal.value = false
    selectedProject.value = null
  } catch (err) {
    console.error('Failed to edit project:', err)
    error.value = 'Failed to update project. Please try again.'
  } finally {
    isLoading.value = false
  }
}

const openEditModal = (event: Event, project: Project) => {
  event.stopPropagation()
  selectedProject.value = { ...project }
  showEditModal.value = true
}

const handleCreateProject = async (projectData: Omit<Project, 'id' | 'ticket_count'>) => {
  try {
    isLoading.value = true
    error.value = null

    const newProject = await projectService.createProject(projectData)
    projects.value.push(newProject)

    showCreateModal.value = false
  } catch (err) {
    console.error('Failed to create project:', err)
    error.value = 'Failed to create project. Please try again.'
  } finally {
    isLoading.value = false
  }
}

const removeProject = async (event: Event, projectId: number) => {
  event.stopPropagation()
  if (!confirm('Are you sure you want to remove this project?')) return

  try {
    isLoading.value = true
    error.value = null

    await projectService.deleteProject(projectId)
    projects.value = projects.value.filter(p => p.id !== projectId)
  } catch (err) {
    console.error('Failed to remove project:', err)
    error.value = 'Failed to delete project. Please try again.'
  } finally {
    isLoading.value = false
  }
}

// Reset filters
const resetFilters = () => {
  searchQuery.value = ''
  statusFilter.value = 'all'
}

// Method to open create modal - can be called from parent (e.g., SiteHeader)
const openCreateModal = () => {
  showCreateModal.value = true
}

// Expose the method so parent components can trigger it
defineExpose({
  openCreateModal
})

// Mobile search bar integration
const { registerMobileSearch, deregisterMobileSearch, updateSearchQuery } = useMobileSearch()

const handleSearchUpdate = (value: string) => {
  searchQuery.value = value
}

const setupMobileSearch = () => {
  registerMobileSearch({
    searchQuery: searchQuery.value,
    placeholder: 'Search projects...',
    showCreateButton: true,
    createIcon: 'folder',
    onSearchUpdate: handleSearchUpdate,
    onCreate: openCreateModal
  })
}

onMounted(setupMobileSearch)
onActivated(setupMobileSearch)
onDeactivated(deregisterMobileSearch)
onUnmounted(deregisterMobileSearch)

// Sync search query changes to mobile search bar
watch(searchQuery, updateSearchQuery)
</script>

<template>
  <div class="flex flex-col h-full overflow-hidden">
    <!-- Header bar with search, filters, and actions -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2 flex-wrap">
        <!-- Search - hidden on mobile (shown in MobileSearchBar) -->
        <DebouncedSearchInput
          v-model="searchQuery"
          placeholder="Search projects..."
          class="hidden sm:block"
        />

        <!-- Status filter -->
        <select
          v-model="statusFilter"
          class="bg-surface-alt border border-default text-primary text-sm rounded-md focus:ring-accent focus:border-accent py-1 px-2 w-[120px]"
        >
          <option
            v-for="option in statusOptions"
            :key="option.value"
            :value="option.value"
          >
            {{ option.label }}
          </option>
        </select>

        <!-- Reset button -->
        <button
          v-if="searchQuery || statusFilter !== 'all'"
          @click="resetFilters"
          class="px-2 py-1 text-xs font-medium text-white bg-accent rounded-md hover:bg-accent-hover focus:ring-2 focus:outline-none focus:ring-accent/50"
        >
          Reset
        </button>

        <!-- Results count -->
        <div class="text-xs text-tertiary ml-auto">
          {{ filteredProjects.length }} project{{ filteredProjects.length !== 1 ? 's' : '' }}
        </div>
      </div>
    </div>

    <!-- Main content -->
    <div class="flex-1 overflow-y-auto p-4">
      <!-- Error message -->
      <ErrorBanner
        v-if="error"
        :message="error"
        :dismissible="true"
        class="mb-4"
        @dismiss="error = null"
      />

      <!-- Skeleton Loading State -->
      <TransitionGroup
        v-if="isLoading"
        name="list-stagger"
        tag="div"
        class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4"
      >
        <div
          v-for="i in 6"
          :key="`skeleton-${i}`"
          :style="getStyle(i - 1)"
          class="bg-surface rounded-xl border border-default overflow-hidden animate-pulse"
        >
          <!-- Skeleton Header -->
          <div class="px-4 py-3 bg-surface-alt border-b border-default">
            <div class="flex items-start gap-3">
              <div class="w-10 h-10 rounded-lg bg-surface-hover"></div>
              <div class="flex-1 min-w-0 space-y-2">
                <div class="h-5 bg-surface-hover rounded w-3/4"></div>
                <div class="h-5 bg-surface-hover rounded w-16"></div>
              </div>
              <div class="flex items-center gap-1.5">
                <div class="w-4 h-4 bg-surface-hover rounded"></div>
                <div class="w-6 h-4 bg-surface-hover rounded"></div>
              </div>
            </div>
          </div>
          <!-- Skeleton Content -->
          <div class="p-4 space-y-3">
            <div class="space-y-2">
              <div class="h-4 bg-surface-hover rounded w-full"></div>
              <div class="h-4 bg-surface-hover rounded w-2/3"></div>
            </div>
            <div class="pt-2 border-t border-subtle flex items-center gap-3">
              <div class="h-3 bg-surface-hover rounded w-16"></div>
              <div class="h-3 bg-surface-hover rounded w-16"></div>
            </div>
          </div>
        </div>
      </TransitionGroup>

      <!-- Empty State -->
      <EmptyState
        v-else-if="filteredProjects.length === 0"
        icon="folder"
        :title="searchQuery || statusFilter !== 'all' ? 'No projects match your filters' : 'No projects found'"
        :description="searchQuery || statusFilter !== 'all' ? 'Try adjusting your search or filters' : 'Create your first project to get started'"
        :action-label="!searchQuery && statusFilter === 'all' ? 'Create Project' : undefined"
        @action="openCreateModal"
      />

      <!-- Projects Grid -->
      <TransitionGroup
        v-else
        name="list-stagger"
        tag="div"
        class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4"
      >
        <div
          v-for="(project, index) in filteredProjects"
          :key="project.id"
          :style="getStyle(index)"
          @click="openProject(project.id)"
          class="bg-surface rounded-xl border border-default overflow-hidden hover:border-strong hover:shadow-lg transition-all cursor-pointer group"
        >
          <!-- Project Header -->
          <div class="px-4 py-3 bg-surface-alt border-b border-default relative">
            <div class="flex items-start gap-3 w-full">
              <!-- Icon -->
              <div class="w-10 h-10 rounded-lg bg-accent-muted flex items-center justify-center flex-shrink-0">
                <svg class="w-5 h-5 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M4 4h4v16H4V4zm6 0h4v12h-4V4zm6 0h4v8h-4V4z" />
                </svg>
              </div>

              <!-- Name and status - grows to fill space -->
              <div class="flex-1 min-w-0">
                <h3 class="font-semibold text-primary truncate group-hover:text-accent transition-colors">
                  {{ project.name }}
                </h3>
                <span
                  class="inline-flex items-center px-2 py-0.5 rounded-md text-xs font-medium border mt-1"
                  :class="getStatusClass(project.status)"
                >
                  {{ formatStatus(project.status) }}
                </span>
              </div>

              <!-- Ticket count - fixed to right -->
              <div class="flex items-center gap-1.5 text-secondary flex-shrink-0">
                <svg class="w-4 h-4 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
                </svg>
                <span class="text-sm font-medium">{{ project.ticket_count || 0 }}</span>
              </div>

              <!-- Action buttons - absolutely positioned on hover -->
              <div class="hidden group-hover:flex items-center gap-1 absolute top-2 right-2 bg-surface-alt rounded-md p-0.5">
                <button
                  @click="(e) => openEditModal(e, project)"
                  class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
                  title="Edit project"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
                  </svg>
                </button>
                <button
                  @click="(e) => removeProject(e, project.id)"
                  class="p-1.5 text-tertiary hover:text-status-error hover:bg-status-error-muted rounded-md transition-colors"
                  title="Delete project"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                  </svg>
                </button>
              </div>
            </div>
          </div>

          <!-- Project Content -->
          <div class="p-4">
            <!-- Description -->
            <p v-if="project.description" class="text-secondary text-sm line-clamp-2">
              {{ project.description }}
            </p>
            <p v-else class="text-tertiary text-sm italic">No description</p>

            <!-- Timestamps -->
            <div class="mt-3 pt-2 border-t border-subtle flex items-center gap-3 text-xs text-tertiary">
              <span class="flex items-center gap-1" :title="'Created ' + project.created_at">
                <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7V3m8 4V3m-9 8h10M5 21h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" />
                </svg>
                {{ formatCompactRelativeTime(project.created_at) }}
              </span>
              <span v-if="project.updated_at !== project.created_at" class="flex items-center gap-1" :title="'Updated ' + project.updated_at">
                <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z" />
                </svg>
                {{ formatCompactRelativeTime(project.updated_at) }}
              </span>
            </div>
          </div>
        </div>
      </TransitionGroup>
    </div>

    <!-- Create Project Modal -->
    <Modal
      :show="showCreateModal"
      title="Create New Project"
      @close="showCreateModal = false"
    >
      <ProjectForm
        mode="create"
        :disabled="isLoading"
        @submit="handleCreateProject"
        @cancel="showCreateModal = false"
      />
    </Modal>

    <!-- Edit Project Modal -->
    <Modal
      :show="showEditModal"
      title="Edit Project"
      @close="showEditModal = false"
    >
      <ProjectForm
        v-if="selectedProject"
        mode="edit"
        :project="selectedProject"
        :disabled="isLoading"
        @submit="handleEditProject"
        @cancel="showEditModal = false"
      />
    </Modal>
  </div>
</template>

