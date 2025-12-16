<script setup lang="ts">
defineProps<{
  type: 'success' | 'error';
  message: string;
}>();
</script>

<template>
  <div
    class="p-4 rounded-xl border mb-4 flex items-start"
    :class="type === 'success'
      ? 'bg-status-success/30 text-status-success border-status-success/50'
      : 'bg-status-error/30 text-status-error border-status-error/50'"
  >
    <div class="mr-3 mt-0.5 flex-shrink-0">
      <!-- Success icon -->
      <svg v-if="type === 'success'" xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
        <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd" />
      </svg>
      <!-- Error icon -->
      <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
        <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
      </svg>
    </div>
    <div>{{ message }}</div>
  </div>
</template>
