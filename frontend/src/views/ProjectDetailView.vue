<!-- ProjectDetailView.vue -->
<script setup lang="ts">
import { ref, onMounted, computed, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import type { Project } from '@/types/project'
import { projectService } from '@/services/projectService'
import Modal from '@/components/Modal.vue'
import ProjectForm from '@/components/projectComponents/ProjectForm.vue'
import AddTicketToProjectModal from '@/components/projectComponents/AddTicketToProjectModal.vue'
import KanbanBoard from '@/components/projectComponents/KanbanBoard.vue'
import ProjectTicketList from '@/components/projectComponents/ProjectTicketList.vue'
import BackButton from '@/components/common/BackButton.vue'
import GanttPlanner from '@/components/projectComponents/GanttPlanner.vue'
import InlineEdit from '@/components/common/InlineEdit.vue'

const route = useRoute()
const router = useRouter()
const projectId = computed(() => Number(route.params.id))

const project = ref<Project | null>(null)
const isLoading = ref(true)
const error = ref<string | null>(null)
const showEditModal = ref(false)
const showAddTicketModal = ref(false)
const ticketListRef = ref<InstanceType<typeof ProjectTicketList> | null>(null)
const existingTicketIds = ref<number[]>([])
const activeTab = computed(() => {
  if (route.query.view === 'list') return 'list'
  if (route.query.view === 'gantt') return 'gantt'
  return 'kanban'
})

// Fetch project details on component mount
onMounted(async () => {
  if (!projectId.value) {
    router.push('/projects')
    return
  }

  try {
    isLoading.value = true
    error.value = null

    // Fetch project details
    project.value = await projectService.getProject(projectId.value)

    // Fetch existing ticket IDs for the modal filter
    await fetchExistingTicketIds()
  } catch (err) {
    console.error('Failed to fetch project details:', err)
    error.value = 'Failed to load project details. Please try again later.'
  } finally {
    isLoading.value = false
  }
})

const fetchExistingTicketIds = async () => {
  if (!projectId.value) return
  try {
    const tickets = await projectService.getProjectTickets(projectId.value)
    existingTicketIds.value = tickets.map((t: { id: number }) => t.id)
  } catch (err) {
    console.error('Failed to fetch existing tickets:', err)
  }
}

const handleEditProject = async (projectData: Omit<Project, 'id' | 'ticket_count'> & { id?: number }) => {
  if (!project.value) return

  try {
    isLoading.value = true
    error.value = null

    const updatedProject = await projectService.updateProject(
      project.value.id,
      projectData
    )

    project.value = updatedProject
    showEditModal.value = false
  } catch (err) {
    console.error('Failed to edit project:', err)
    error.value = 'Failed to update project. Please try again.'
  } finally {
    isLoading.value = false
  }
}

const handleTitleUpdate = async (newTitle: string) => {
  if (!project.value || newTitle === project.value.name) return

  try {
    error.value = null
    const updatedProject = await projectService.updateProject(
      project.value.id,
      { ...project.value, name: newTitle }
    )
    project.value = updatedProject
  } catch (err) {
    console.error('Failed to update project title:', err)
    error.value = 'Failed to update title. Please try again.'
  }
}

const handleDeleteProject = async () => {
  if (!project.value) return
  
  if (!confirm(`Are you sure you want to delete the project "${project.value.name}"?`)) {
    return
  }
  
  try {
    isLoading.value = true
    error.value = null
    
    await projectService.deleteProject(project.value.id)
    router.push('/projects')
  } catch (err) {
    console.error('Failed to delete project:', err)
    error.value = 'Failed to delete project. Please try again.'
  } finally {
    isLoading.value = false
  }
}

const handleAddTicketComplete = async () => {
  // Refresh the ticket list and existing IDs
  ticketListRef.value?.refresh()
  await fetchExistingTicketIds()

  // Update project to get new ticket count
  if (project.value) {
    project.value = await projectService.getProject(project.value.id)
  }
}

const handleRemoveTicket = async (ticketId: number) => {
  if (!project.value) return

  if (!confirm('Are you sure you want to remove this ticket from the project?')) {
    return
  }

  try {
    error.value = null
    await projectService.removeTicketFromProject(project.value.id, ticketId)

    // Refresh the ticket list
    ticketListRef.value?.refresh()
    await fetchExistingTicketIds()

    // Update project to get new ticket count
    project.value = await projectService.getProject(project.value.id)
  } catch (err) {
    console.error('Failed to remove ticket from project:', err)
    error.value = 'Failed to remove ticket from project. Please try again.'
  }
}

const handleTicketCountChange = (count: number) => {
  if (project.value) {
    project.value = { ...project.value, ticket_count: count }
  }
}

const getStatusClass = (status: string) => {
  switch (status) {
    case 'active':
      return 'bg-status-success/20 text-status-success border-status-success/30'
    case 'completed':
      return 'bg-accent/20 text-accent border-accent/30'
    case 'archived':
      return 'bg-surface-alt/20 text-secondary border-surface-alt/30'
    default:
      return 'bg-surface-alt/20 text-secondary border-surface-alt/30'
  }
}


// Update URL when view changes
const setActiveTab = (tab: string) => {
  router.replace({ 
    query: { 
      ...route.query,
      view: tab === 'kanban' ? undefined : tab 
    } 
  })
}

// Watch for route query changes to sync the active tab
watch(() => route.query.view, (newValue) => {
  // No need to set activeTab since it's now computed from the route
  console.log(`View changed to ${newValue || 'kanban'}`)
}, { immediate: true })

// Method to open add ticket modal from SiteHeader
const openAddTicketModal = () => {
  showAddTicketModal.value = true
}

// Expose methods for parent component access (SiteHeader create button)
defineExpose({
  openAddTicketModal
})
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Header section - compact two-row layout -->
    <div class="flex-shrink-0 bg-surface border-b border-default">
      <!-- Error message -->
      <div v-if="error" class="mx-2 sm:mx-4 mt-2 bg-status-error/20 border border-status-error/50 text-status-error px-3 py-2 rounded-lg text-sm">
        {{ error }}
      </div>

      <!-- Loading state -->
      <div v-if="isLoading" class="flex justify-center items-center py-4">
        <div class="animate-spin rounded-full h-5 w-5 border-b-2 border-accent"></div>
      </div>

      <template v-else-if="project">
        <!-- Row 1: Back + Title + Status + Ticket count + Actions -->
        <div class="flex items-center gap-2 px-2 sm:px-4 py-2">
          <BackButton fallbackRoute="/projects" label="Projects" compact />

          <div class="min-w-0 flex-1 flex items-center gap-2">
            <InlineEdit
              :modelValue="project.name"
              placeholder="Project name..."
              text-size="lg"
              :show-edit-hint="false"
              :truncate="true"
              @update:modelValue="handleTitleUpdate"
            />
          </div>

          <!-- Status + Ticket count + Actions -->
          <div class="flex items-center gap-2 flex-shrink-0">
            <span
              :class="getStatusClass(project.status)"
              class="px-1.5 py-0.5 rounded text-[11px] font-medium border capitalize"
            >
              {{ project.status }}
            </span>
            <span class="text-xs text-tertiary">•</span>
            <div class="flex items-center gap-1 text-xs text-tertiary">
              <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
              </svg>
              <span>{{ project.ticket_count || 0 }}</span>
            </div>
            <div class="flex items-center gap-0.5 ml-1">
              <button
                @click="showEditModal = true"
                class="p-1.5 text-tertiary hover:text-primary transition-colors rounded-md hover:bg-surface-hover"
                title="Edit project"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
                  <path d="M13.586 3.586a2 2 0 112.828 2.828l-.793.793-2.828-2.828.793-.793zM11.379 5.793L3 14.172V17h2.828l8.38-8.379-2.83-2.828z" />
                </svg>
              </button>
              <button
                @click="handleDeleteProject"
                class="p-1.5 text-tertiary hover:text-status-error transition-colors rounded-md hover:bg-surface-hover"
                title="Delete project"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
                  <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
                </svg>
              </button>
            </div>
          </div>
        </div>

        <!-- Row 2: Tabs + Description -->
        <div class="flex items-center justify-between gap-4 px-2 sm:px-4 border-t border-subtle">
          <!-- Tabs -->
          <div class="flex gap-0.5 flex-shrink-0">
            <button
              @click="setActiveTab('kanban')"
              class="py-1.5 px-3 text-sm font-medium transition-colors border-b-2 -mb-px"
              :class="activeTab === 'kanban' ? 'border-accent text-accent' : 'border-transparent text-tertiary hover:text-secondary'"
            >
              Kanban
            </button>
            <button
              @click="setActiveTab('list')"
              class="py-1.5 px-3 text-sm font-medium transition-colors border-b-2 -mb-px"
              :class="activeTab === 'list' ? 'border-accent text-accent' : 'border-transparent text-tertiary hover:text-secondary'"
            >
              List
            </button>
          </div>
          <!-- Description (inline, truncated) -->
          <p v-if="project.description" class="text-xs text-tertiary truncate min-w-0 py-1.5">
            {{ project.description }}
          </p>
        </div>
      </template>
    </div>

    <!-- Kanban Board View - fills remaining height with scroll -->
    <div v-if="!isLoading && project && activeTab === 'kanban'" class="flex-1 min-h-0 overflow-auto">
      <KanbanBoard :project-id="project.id" />
    </div>

    <!-- Gantt Planner View -->
    <div v-else-if="!isLoading && project && activeTab === 'gantt'" class="flex-1 min-h-[500px] px-4 md:px-6">
      <GanttPlanner :project-id="project.id" :tickets="[]" />
    </div>

    <!-- List View -->
    <div v-else-if="!isLoading && project && activeTab === 'list'" class="flex-1 flex flex-col min-h-0 overflow-auto">
      <ProjectTicketList
        ref="ticketListRef"
        :project-id="project.id"
        @add-ticket="showAddTicketModal = true"
        @remove-ticket="handleRemoveTicket"
        @ticket-count-change="handleTicketCountChange"
      />
    </div>

    <!-- Edit Project Modal -->
    <Modal
      :show="showEditModal"
      title="Edit Project"
      @close="showEditModal = false"
    >
      <ProjectForm
        v-if="project"
        mode="edit"
        :project="project"
        :disabled="isLoading"
        @submit="handleEditProject"
        @cancel="showEditModal = false"
      />
    </Modal>

    <!-- Add Ticket Modal -->
    <AddTicketToProjectModal
      v-if="project"
      :show="showAddTicketModal"
      :project-id="project.id"
      :existing-tickets="existingTicketIds"
      @close="showAddTicketModal = false"
      @add-ticket="handleAddTicketComplete"
      @refresh="handleAddTicketComplete"
    />
  </div>
</template> 