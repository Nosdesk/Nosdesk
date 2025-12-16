<!-- ProjectDetailView.vue -->
<script setup lang="ts">
import { ref, onMounted, computed, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import type { Project } from '@/types/project'
import { projectService } from '@/services/projectService'
import Modal from '@/components/Modal.vue'
import ProjectForm from '@/components/projectComponents/ProjectForm.vue'
import TicketSelectionModal from '@/components/projectComponents/TicketSelectionModal.vue'
import KanbanBoard from '@/components/projectComponents/KanbanBoard.vue'
import StatusBadge from '@/components/StatusBadge.vue'
import UserAvatar from '@/components/UserAvatar.vue'
import BackButton from '@/components/common/BackButton.vue'
import GanttPlanner from '@/components/projectComponents/GanttPlanner.vue'

const route = useRoute()
const router = useRouter()
const projectId = computed(() => Number(route.params.id))

const project = ref<Project | null>(null)
const tickets = ref<any[]>([])
const isLoading = ref(true)
const isTicketsLoading = ref(false)
const error = ref<string | null>(null)
const showEditModal = ref(false)
const showAddTicketModal = ref(false)
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
    
    // Fetch project tickets
    await fetchProjectTickets()
  } catch (err) {
    console.error('Failed to fetch project details:', err)
    error.value = 'Failed to load project details. Please try again later.'
  } finally {
    isLoading.value = false
  }
})

const fetchProjectTickets = async () => {
  if (!projectId.value) return
  
  try {
    isTicketsLoading.value = true
    tickets.value = await projectService.getProjectTickets(projectId.value)
  } catch (err) {
    console.error('Failed to fetch project tickets:', err)
    error.value = 'Failed to load project tickets. Please try again later.'
  } finally {
    isTicketsLoading.value = false
  }
}

const handleEditProject = async (projectData: Omit<Project, 'id' | 'ticketCount'> & { id?: number }) => {
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

const handleAddTicket = async (ticketId: number) => {
  if (!project.value) return
  
  try {
    error.value = null
    
    await projectService.addTicketToProject(project.value.id, ticketId)
    
    // Refresh the ticket list
    await fetchProjectTickets()
    
    // Update the project to get the new ticket count
    project.value = await projectService.getProject(project.value.id)
    
    showAddTicketModal.value = false
  } catch (err) {
    console.error('Failed to add ticket to project:', err)
    error.value = 'Failed to add ticket to project. Please try again.'
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
    await fetchProjectTickets()
    
    // Update the project to get the new ticket count
    project.value = await projectService.getProject(project.value.id)
  } catch (err) {
    console.error('Failed to remove ticket from project:', err)
    error.value = 'Failed to remove ticket from project. Please try again.'
  }
}

const goToTicket = (ticketId: number) => {
  router.push(`/tickets/${ticketId}`)
}

const getStatusClass = (status: string) => {
  switch (status) {
    case 'active':
      return 'bg-green-500/20 text-green-400 border-green-500/30'
    case 'completed':
      return 'bg-blue-500/20 text-blue-400 border-blue-500/30'
    case 'archived':
      return 'bg-gray-500/20 text-gray-400 border-gray-500/30'
    default:
      return 'bg-gray-500/20 text-gray-400 border-gray-500/30'
  }
}

const getPriorityClass = (priority: string) => {
  switch (priority) {
    case 'high':
      return 'bg-red-400/20 dark:bg-red-500/20 [color:#7f1d1d] dark:text-red-200 border-red-400/40 dark:border-red-500/30'
    case 'medium':
      return 'bg-amber-400/20 dark:bg-amber-500/20 [color:#78350f] dark:text-amber-200 border-amber-400/40 dark:border-amber-500/30'
    case 'low':
      return 'bg-blue-400/20 dark:bg-blue-500/20 [color:#1e3a8a] dark:text-blue-200 border-blue-400/40 dark:border-blue-500/30'
    default:
      return 'bg-gray-400/20 dark:bg-gray-500/20 text-gray-900 dark:text-gray-200 border-gray-400/40 dark:border-gray-500/30'
  }
}

// Get existing ticket IDs for filtering in the ticket selection modal
const existingTicketIds = computed(() => tickets.value.map(ticket => ticket.id))

// Format date helper function
const formatDate = (dateString: string): string => {
  try {
    const date = new Date(dateString);
    const now = new Date();
    const diffTime = now.getTime() - date.getTime();
    const diffMinutes = Math.floor(diffTime / (1000 * 60));

    if (diffMinutes < 1) {
      return 'just now';
    } else if (diffMinutes < 60) {
      return `${diffMinutes}m ago`;
    } else if (diffMinutes < 1440) {
      const hours = Math.floor(diffMinutes / 60);
      return `${hours}h ago`;
    } else {
      const days = Math.floor(diffMinutes / 1440);
      return `${days}d ago`;
    }
  } catch (e) {
    return 'unknown';
  }
}

// Get ticket updated date from various possible field names
const getTicketUpdatedDate = (ticket: any): string => {
  const possibleFields = ['updated_at', 'updatedAt', 'updated', 'lastUpdated', 'modifiedAt'];
  
  for (const field of possibleFields) {
    if (ticket[field]) {
      return formatDate(ticket[field]);
    }
  }
  
  // Fallback to created date if available
  if (ticket.created_at || ticket.createdAt || ticket.created) {
    const dateField = ticket.created_at || ticket.createdAt || ticket.created;
    return formatDate(dateField);
  }
  
  return 'N/A';
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
</script>

<template>
  <div class="flex flex-col h-full">
    <div class="flex flex-col gap-4 p-6">
      <!-- Back button -->
      <BackButton fallbackRoute="/projects" label="Back to Projects" />

      <!-- Error message -->
      <div v-if="error" class="bg-status-error/20 border border-status-error/50 text-red-200 px-4 py-3 rounded-lg mb-4">
        {{ error }}
      </div>

      <!-- Loading state -->
      <div v-if="isLoading" class="flex justify-center items-center py-8">
        <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-brand-blue"></div>
      </div>

      <!-- Project details -->
      <div v-else-if="project" class="flex flex-col gap-4">
        <!-- Project header -->
        <div class="flex justify-between items-start">
          <div>
            <h1 class="text-2xl font-semibold text-primary">{{ project.name }}</h1>
            <p class="text-secondary mt-2">{{ project.description }}</p>
          </div>
          <div class="flex items-center gap-2">
            <span 
              :class="getStatusClass(project.status)"
              class="px-3 py-1 rounded-full text-sm border"
            >
              {{ project.status }}
            </span>
            <button
              @click="showEditModal = true"
              class="p-2 text-secondary hover:text-primary transition-colors"
              title="Edit project"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
                <path d="M13.586 3.586a2 2 0 112.828 2.828l-.793.793-2.828-2.828.793-.793zM11.379 5.793L3 14.172V17h2.828l8.38-8.379-2.83-2.828z" />
              </svg>
            </button>
            <button
              @click="handleDeleteProject"
              class="p-2 text-secondary hover:text-status-error transition-colors"
              title="Delete project"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
                <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
              </svg>
            </button>
          </div>
        </div>

        <!-- Project view tabs -->
        <div class="border-b border-default">
          <div class="flex gap-4">
            <button
              @click="setActiveTab('kanban')"
              class="py-2 px-4 border-b-2 font-medium text-sm"
              :class="activeTab === 'kanban' ? 'border-brand-blue text-brand-blue' : 'border-transparent text-secondary hover:text-primary'"
            >
              Kanban Board
            </button>
            <button
              @click="setActiveTab('list')"
              class="py-2 px-4 border-b-2 font-medium text-sm"
              :class="activeTab === 'list' ? 'border-brand-blue text-brand-blue' : 'border-transparent text-secondary hover:text-primary'"
            >
              List View
            </button>
            <!-- Gantt Planner to be implemented in a future release -->
            <!-- <button 
              @click="setActiveTab('gantt')"
              class="py-2 px-4 border-b-2 font-medium text-sm"
              :class="activeTab === 'gantt' ? 'border-blue-500 text-blue-500' : 'border-transparent text-slate-400 hover:text-slate-300'"
            >
              Gantt Planner
            </button> -->
          </div>
        </div>

        <!-- Kanban Board View -->
        <div v-if="activeTab === 'kanban'" class="flex-1 min-h-[500px]">
          <KanbanBoard :project-id="project.id" />
        </div>

        <!-- Gantt Planner View -->
        <div v-else-if="activeTab === 'gantt'" class="flex-1 min-h-[500px]">
          <GanttPlanner v-if="project" :project-id="project.id" :tickets="tickets" />
        </div>

        <!-- List View -->
        <div v-else class="flex flex-col gap-4">
          <div class="flex justify-between items-center mb-4">
            <h2 class="text-xl font-medium text-white">Tickets</h2>
            <button 
              @click="showAddTicketModal = true"
              class="flex items-center gap-2 px-4 py-2 bg-brand-blue text-white text-sm rounded-lg hover:opacity-90 transition-colors"
            >
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
              </svg>
              Add Ticket
            </button>
          </div>
          
          <div v-if="isTicketsLoading" class="text-center py-8 text-secondary">
            <div class="inline-flex items-center gap-3">
              <svg class="w-5 h-5 animate-spin" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              <span>Loading tickets...</span>
            </div>
          </div>
          
          <div v-else-if="tickets.length === 0" class="text-center py-12 text-secondary">
            <div class="inline-flex flex-col items-center gap-4">
              <svg class="w-16 h-16 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v10a2 2 0 002 2h8a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
              </svg>
              <div class="text-center">
                <p class="text-lg font-medium text-primary">No tickets in this project</p>
                <p class="text-sm text-tertiary mt-1">Add tickets to get started with project management</p>
              </div>
              <button 
                @click="showAddTicketModal = true"
                class="mt-2 px-6 py-2 bg-brand-blue text-white rounded-lg hover:opacity-90 transition-colors"
              >
                Add Your First Ticket
              </button>
            </div>
          </div>
          
          <div v-else class="bg-surface rounded-lg border border-default overflow-hidden">
            <!-- Table header -->
            <div class="bg-surface-alt px-4 py-3 border-b border-default sticky top-0 z-10">
              <div class="grid grid-cols-12 gap-3 text-xs font-medium text-primary uppercase tracking-wide">
                <div class="col-span-1">ID</div>
                <div class="col-span-3">Title</div>
                <div class="col-span-1">Status</div>
                <div class="col-span-1">Priority</div>
                <div class="col-span-3">Assignee</div>
                <div class="col-span-1">Updated</div>
                <div class="col-span-2 text-right">Actions</div>
              </div>
            </div>
            
            <!-- Ticket rows -->
            <div class="divide-y divide-subtle">
              <div
                v-for="ticket in tickets"
                :key="ticket.id"
                class="group relative hover:bg-surface-hover transition-colors duration-150"
              >
                <div class="px-4 py-3">
                  <div class="grid grid-cols-12 gap-3 items-center">
                    <!-- Ticket ID -->
                    <div class="col-span-1 min-w-0">
                      <span class="text-sm font-mono text-secondary">#{{ ticket.id }}</span>
                    </div>

                    <!-- Title -->
                    <div class="col-span-3 min-w-0">
                      <div 
                        @click="goToTicket(ticket.id)"
                        class="cursor-pointer hover:underline"
                      >
                        <div class="font-medium text-primary truncate text-sm">{{ ticket.title }}</div>
                        <div v-if="ticket.description" class="text-xs text-secondary truncate mt-1">{{ ticket.description }}</div>
                      </div>
                    </div>

                    <!-- Status -->
                    <div class="col-span-1 min-w-0">
                      <StatusBadge 
                        type="status" 
                        :value="ticket.status"
                      />
                    </div>

                    <!-- Priority -->
                    <div class="col-span-1 min-w-0">
                      <StatusBadge 
                        type="priority"
                        class="text-sm"
                        :value="ticket.priority"
                        short
                      />
                    </div>

                    <!-- Assignee -->
                    <div class="col-span-3 min-w-0">
                      <div v-if="ticket.assignee" class="flex items-center gap-2">
                        <UserAvatar 
                          :name="ticket.assignee" 
                          :avatarUrl="ticket.assignee_avatar"
                          size="sm" 
                          :show-name="true"
                          :clickable="false"
                        />
                      </div>
                      <div v-else class="flex items-center gap-2 text-tertiary">
                        <div class="w-6 h-6 rounded-full bg-surface-alt flex items-center justify-center">
                          <svg class="w-3 h-3" fill="currentColor" viewBox="0 0 20 20">
                            <path fill-rule="evenodd" d="M10 9a3 3 0 100-6 3 3 0 000 6zm-7 9a7 7 0 1114 0H3z" clip-rule="evenodd" />
                          </svg>
                        </div>
                        <span class="text-xs">Unassigned</span>
                      </div>
                    </div>

                    <!-- Updated -->
                    <div class="col-span-1 min-w-0">
                      <span class="text-xs text-secondary">
                        {{ getTicketUpdatedDate(ticket) }}
                      </span>
                    </div>

                    <!-- Actions -->
                    <div class="col-span-2 text-right">
                      <div class="flex items-center justify-end gap-1">
                        <button 
                          @click="goToTicket(ticket.id)"
                          class="text-blue-400 hover:text-blue-300 text-xs font-medium px-2 py-1 rounded hover:bg-blue-900/20 transition-colors"
                          title="View ticket"
                        >
                          View
                        </button>
                        <button 
                          @click="handleRemoveTicket(ticket.id)"
                          class="text-red-400 hover:text-red-300 text-xs font-medium px-2 py-1 rounded hover:bg-red-900/20 transition-colors"
                          title="Remove from project"
                        >
                          <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 20 20">
                            <path fill-rule="evenodd" d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z" clip-rule="evenodd" />
                          </svg>
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
            
            <!-- Footer -->
            <div class="p-3 text-center border-t border-subtle bg-surface-alt">
              <span class="text-xs text-tertiary">
                {{ tickets.length }} ticket{{ tickets.length !== 1 ? 's' : '' }} in this project
              </span>
            </div>
          </div>
        </div>
      </div>
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
    <TicketSelectionModal
      v-if="project"
      :show="showAddTicketModal"
      :project-id="project.id"
      :existing-ticket-ids="existingTicketIds"
      @close="showAddTicketModal = false"
      @select-ticket="handleAddTicket"
    />
  </div>
</template> 