<script setup lang="ts">
/**
 * Plugin Loading Component
 *
 * Displayed while a plugin component is being loaded.
 */
</script>

<template>
  <div class="plugin-loading p-4 flex items-center justify-center">
    <div class="flex items-center gap-2 text-secondary">
      <svg
        class="animate-spin w-4 h-4"
        fill="none"
        viewBox="0 0 24 24"
      >
        <circle
          class="opacity-25"
          cx="12"
          cy="12"
          r="10"
          stroke="currentColor"
          stroke-width="4"
        />
        <path
          class="opacity-75"
          fill="currentColor"
          d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
        />
      </svg>
      <span class="text-sm">Loading plugin...</span>
    </div>
  </div>
</template>

<style scoped>
.plugin-loading {
  min-height: 60px;
  background: var(--surface-alt);
  border-radius: 0.5rem;
}
</style>
