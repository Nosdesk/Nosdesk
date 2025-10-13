<!-- ProjectsView.vue -->
<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import type { Project } from '@/types/project'
import Modal from '@/components/Modal.vue'
import ProjectForm from '@/components/projectComponents/ProjectForm.vue'
import { projectService } from '@/services/projectService'

const projects = ref<Project[]>([])
const router = useRouter()
const showCreateModal = ref(false)
const showEditModal = ref(false)
const isLoading = ref(true)
const error = ref<string | null>(null)
const selectedProject = ref<Project | null>(null)

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

const handleEditProject = async (projectData: Omit<Project, 'id' | 'ticketCount'> & { id?: number }) => {
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

const handleCreateProject = async (projectData: Omit<Project, 'id' | 'ticketCount'>) => {
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

// Method to open create modal - can be called from parent (e.g., SiteHeader)
const openCreateModal = () => {
  showCreateModal.value = true
}

// Expose the method so parent components can trigger it
defineExpose({
  openCreateModal
})
</script>

<template>
  <div class="flex flex-col h-full">
    <div class="flex flex-col gap-4 p-6">
      <!-- Error message -->
      <div v-if="error" class="bg-red-900/30 border border-red-700/30 text-red-400 px-4 py-3 rounded-lg">
        {{ error }}
      </div>

      <!-- Loading State -->
      <div v-if="isLoading" class="flex justify-center items-center py-8">
        <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500"></div>
      </div>

      <!-- Empty State -->
      <div v-else-if="projects.length === 0" class="bg-slate-800 rounded-xl border border-slate-700/50 p-8 text-center">
        <p class="text-slate-400">No projects found. Create your first project to get started.</p>
      </div>

      <!-- Projects Grid -->
      <div v-else class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        <div
          v-for="project in projects"
          @click="openProject(project.id)" 
          :key="project.id"
          class="bg-slate-800 rounded-xl border border-slate-700/50 overflow-hidden hover:border-slate-600/50 transition-colors cursor-pointer"
        >
          <!-- Project Header -->
          <div class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3 min-w-0 flex-1">
                <div class="w-2 h-2 bg-blue-500 rounded-full flex-shrink-0"></div>
                <h3 class="font-medium text-white truncate">{{ project.name }}</h3>
                <div 
                  class="px-2 py-1 rounded-md text-xs font-medium border flex-shrink-0"
                  :class="{
                    'bg-green-900/30 text-green-400 border-green-700/30': project.status === 'active',
                    'bg-blue-900/30 text-blue-400 border-blue-700/30': project.status === 'completed',
                    'bg-slate-900/30 text-slate-400 border-slate-700/30': project.status === 'archived'
                  }"
                >
                  {{ project.status }}
                </div>
              </div>
              
              <!-- Action buttons -->
              <div class="flex items-center gap-1 ml-2">
                <button 
                  @click="(e) => openEditModal(e, project)" 
                  class="p-1.5 text-slate-400 hover:text-white hover:bg-slate-600 rounded-md transition-colors"
                  title="Edit project"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
                  </svg>
                </button>
                <button 
                  @click="(e) => removeProject(e, project.id)" 
                  class="p-1.5 text-slate-400 hover:text-red-400 hover:bg-red-900/20 rounded-md transition-colors"
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
            <div class="flex flex-col gap-3">
              <p class="text-slate-400 text-sm line-clamp-2">{{ project.description }}</p>
              
              <!-- Project Stats -->
              <div class="pt-2 border-t border-slate-700/50">
                <div class="flex items-center gap-2 text-sm text-slate-400">
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
                  </svg>
                  {{ project.ticketCount }} tickets
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
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