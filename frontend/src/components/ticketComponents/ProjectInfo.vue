<!-- components/ProjectInfo.vue -->
<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import type { Project } from '@/services/ticketService';
import { projectService } from '@/services/projectService';

const props = defineProps<{
  projectId: string | number;
}>();

const emit = defineEmits<{
  (e: 'remove'): void;
  (e: 'view'): void;
}>();

// Local state for project data
const project = ref<Project | null>(null);
const isLoading = ref(false);

// Fetch project data on mount
const fetchProject = async () => {
  try {
    isLoading.value = true;
    const fetchedProject = await projectService.getProject(Number(props.projectId));
    if (fetchedProject) {
      project.value = fetchedProject;
    }
  } catch (error) {
    console.error(`Error fetching project #${props.projectId}:`, error);
  } finally {
    isLoading.value = false;
  }
};

// Compute ticket count from project data or use a default value
const ticketCount = computed(() => {
  if (!project.value) return '—';
  // Check if the project has a ticket_count property (from API)
  if ('ticket_count' in project.value) {
    return (project.value as any).ticket_count;
  }
  return project.value.ticketCount || '—';
});

onMounted(() => {
  fetchProject();
});

const getStatusClass = (status: string) => {
  switch (status) {
    case 'active':
      return 'bg-status-success/20 text-status-success border-status-success/30';
    case 'completed':
      return 'bg-accent/20 text-accent border-accent/30';
    case 'archived':
      return 'bg-surface-alt text-secondary border-default';
    default:
      return 'bg-surface-alt text-secondary border-default';
  }
};
</script>

<template>
  <!-- Screen-only loading state -->
  <div v-if="isLoading" class="print:hidden bg-surface rounded-xl border border-default p-4">
    <div class="animate-pulse flex flex-col gap-3">
      <div class="h-6 bg-surface-alt rounded w-1/2"></div>
      <div class="h-4 bg-surface-alt rounded w-3/4"></div>
      <div class="h-4 bg-surface-alt rounded w-1/3"></div>
    </div>
  </div>

  <!-- Screen-only interactive layout -->
  <div v-else-if="project" class="print:hidden bg-surface rounded-xl border border-default overflow-hidden hover:border-strong transition-colors">
    <!-- Header -->
    <div class="px-4 py-3 bg-surface-alt border-b border-default">
      <div class="flex items-center justify-between">
        <h3
          @click="emit('view')"
          class="text-md font-medium text-primary truncate cursor-pointer hover:text-accent transition-colors"
        >
          {{ project.name }}
        </h3>
        
        <!-- Action button -->
        <button
          @click="emit('remove')"
          class="p-1.5 text-tertiary hover:text-status-error hover:bg-status-error/20 rounded-md transition-colors"
          title="Remove from project"
        >
          <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z" clip-rule="evenodd" />
          </svg>
        </button>
      </div>
    </div>
    
    <!-- Content -->
    <div class="p-4">
      <div class="flex flex-col gap-3">
        <!-- Description -->
        <div v-if="project.description" class="flex flex-col gap-1">
          <span class="text-xs text-tertiary uppercase tracking-wide">Description</span>
          <p class="text-sm text-secondary">{{ project.description }}</p>
        </div>

        <!-- Project metadata -->
        <div class="grid grid-cols-3 gap-3 text-sm">
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary uppercase tracking-wide">Project ID</span>
            <span class="text-secondary font-mono text-sm">#{{ projectId }}</span>
          </div>
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary uppercase tracking-wide">Status</span>
            <span
              :class="getStatusClass(project.status)"
              class="text-sm px-2 py-1 rounded-md border w-fit"
            >
              {{ project.status }}
            </span>
          </div>
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary uppercase tracking-wide">Tickets</span>
            <span class="text-secondary text-sm">{{ ticketCount }}</span>
          </div>
        </div>
      </div>
    </div>
  </div>

  <!-- Print-only compact layout -->
  <div v-if="project" class="hidden print:block print-project-card">
    <div class="print-project-header">
      <span class="print-project-name">{{ project.name }}</span>
      <span class="print-project-status" :class="`print-status-${project.status}`">{{ project.status }}</span>
      <span class="print-project-meta">
        <span class="print-project-id">#{{ projectId }}</span>
        <span v-if="ticketCount !== '—'">{{ ticketCount }} tickets</span>
      </span>
    </div>
    <p v-if="project.description" class="print-project-description">{{ project.description }}</p>
  </div>
</template>

<style scoped>
@media print {
  .print-project-card {
    border: 1px solid #ccc;
    padding: 6pt 8pt;
    margin-bottom: 4pt;
    background: #fafafa;
    font-size: 9pt;
    display: flex;
    flex-direction: column;
    gap: 4pt;
  }

  .print-project-header {
    display: flex;
    align-items: center;
    gap: 8pt;
    flex-wrap: wrap;
  }

  .print-project-name {
    font-weight: 600;
    color: #000;
  }

  .print-project-status {
    font-size: 8pt;
    padding: 1pt 4pt;
    border: 1px solid currentColor;
    border-radius: 2pt;
    text-transform: capitalize;
  }

  .print-status-active {
    color: #047857;
  }

  .print-status-completed {
    color: #1d4ed8;
  }

  .print-status-archived {
    color: #666;
  }

  .print-project-meta {
    display: flex;
    align-items: center;
    gap: 8pt;
    color: #666;
    font-size: 8pt;
    margin-left: auto;
  }

  .print-project-id {
    font-family: ui-monospace, monospace;
  }

  .print-project-description {
    color: #333;
    margin: 0;
    font-size: 8pt;
    line-height: 1.3;
  }
}
</style>