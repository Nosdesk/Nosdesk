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
  (e: 'submit', project: Omit<Project, 'id' | 'ticket_count'> & { id?: number }): void;
  (e: 'cancel'): void;
}>()

const formData = ref<Omit<Project, 'id' | 'ticket_count'> & { id?: number }>({
  name: '',
  description: undefined,
  status: 'active'
})

// Initialize form data if editing
watch(() => props.project, (newProject) => {
  if (newProject) {
    const { ticket_count, ...rest } = newProject
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

// Common input classes
const inputClasses = 'w-full px-3 py-2.5 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-accent transition-colors disabled:opacity-50 disabled:cursor-not-allowed'
</script>

<template>
  <form @submit.prevent="handleSubmit" class="flex flex-col gap-5">
    <!-- Project Name -->
    <div class="flex flex-col gap-1.5">
      <label for="name" class="text-sm font-medium text-primary">
        Project Name <span class="text-status-error">*</span>
      </label>
      <input
        id="name"
        v-model="formData.name"
        type="text"
        required
        :disabled="disabled"
        :class="inputClasses"
        placeholder="Enter project name"
        autocomplete="off"
      />
    </div>

    <!-- Project Description -->
    <div class="flex flex-col gap-1.5">
      <label for="description" class="text-sm font-medium text-primary">
        Description
        <span class="text-tertiary font-normal">(optional)</span>
      </label>
      <textarea
        id="description"
        v-model="formData.description"
        rows="3"
        :disabled="disabled"
        :class="[inputClasses, 'resize-none']"
        placeholder="Brief description of the project"
      ></textarea>
    </div>

    <!-- Project Status (only show in edit mode) -->
    <div v-if="mode === 'edit'" class="flex flex-col gap-1.5">
      <label for="status" class="text-sm font-medium text-primary">Status</label>
      <select
        id="status"
        v-model="formData.status"
        :disabled="disabled"
        :class="inputClasses"
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
    <div class="flex flex-col-reverse sm:flex-row justify-end gap-2 pt-2">
      <button
        type="button"
        @click="emit('cancel')"
        :disabled="disabled"
        class="px-4 py-2.5 text-sm font-medium text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors disabled:opacity-50"
      >
        Cancel
      </button>
      <button
        type="submit"
        :disabled="disabled || !formData.name"
        class="px-4 py-2.5 text-sm font-medium bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
      >
        {{ mode === 'create' ? 'Create Project' : 'Save Changes' }}
      </button>
    </div>
  </form>
</template> 