<!-- ProjectForm.vue -->
<script setup lang="ts">
import { ref, watch } from 'vue'
import type { Project } from '@/types/project'

const props = defineProps<{
  project?: Project;
  mode: 'create' | 'edit';
  disabled?: boolean;
}>()

const emit = defineEmits<{
  (e: 'submit', project: Omit<Project, 'id' | 'ticketCount'> & { id?: number }): void;
  (e: 'cancel'): void;
}>()

const formData = ref<Omit<Project, 'id' | 'ticketCount'> & { id?: number }>({
  name: '',
  description: undefined,
  status: 'active'
})

// Initialize form data if editing
watch(() => props.project, (newProject) => {
  if (newProject) {
    const { ticketCount, ...rest } = newProject
    formData.value = { ...rest }
  }
}, { immediate: true })

const handleSubmit = (e: Event) => {
  e.preventDefault();
  if (!formData.value.name) return;
  emit('submit', formData.value);
}

const statusOptions = [
  { value: 'active', label: 'Active' },
  { value: 'completed', label: 'Completed' },
  { value: 'archived', label: 'Archived' }
]
</script>

<template>
  <form @submit.prevent="handleSubmit" class="flex flex-col gap-6">
    <!-- Project Name -->
    <div class="flex flex-col gap-2">
      <label for="name" class="text-sm font-medium text-secondary">Project Name</label>
      <input
        id="name"
        v-model="formData.name"
        type="text"
        required
        :disabled="disabled"
        class="px-3 py-2 bg-surface rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent disabled:opacity-50"
        placeholder="Enter project name"
      />
    </div>

    <!-- Project Description -->
    <div class="flex flex-col gap-2">
      <label for="description" class="text-sm font-medium text-secondary">Description (optional)</label>
      <textarea
        id="description"
        v-model="formData.description"
        rows="3"
        :disabled="disabled"
        class="px-3 py-2 bg-surface rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent resize-none disabled:opacity-50"
        placeholder="Enter project description (optional)"
      ></textarea>
    </div>

    <!-- Project Status (only show in edit mode) -->
    <div v-if="mode === 'edit'" class="flex flex-col gap-2">
      <label for="status" class="text-sm font-medium text-secondary">Status</label>
      <select
        id="status"
        v-model="formData.status"
        :disabled="disabled"
        class="px-3 py-2 bg-surface rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent disabled:opacity-50"
      >
        <option
          v-for="option in statusOptions"
          :key="option.value"
          :value="option.value"
        >
          {{ option.label }}
        </option>
      </select>
    </div>

    <!-- Form Actions -->
    <div class="flex justify-end gap-3">
      <button
        type="button"
        @click="emit('cancel')"
        :disabled="disabled"
        class="px-4 py-2 text-sm font-medium text-secondary hover:text-primary transition-colors disabled:opacity-50"
      >
        Cancel
      </button>
      <button
        type="submit"
        :disabled="disabled"
        class="px-4 py-2 text-sm font-medium bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50"
      >
        {{ mode === 'create' ? 'Create Project' : 'Save Changes' }}
      </button>
    </div>
  </form>
</template> 