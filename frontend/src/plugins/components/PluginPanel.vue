<script setup lang="ts">
/**
 * Plugin Panel Component
 *
 * A container component for plugin content with consistent styling.
 * Provides a title, optional icon, and collapsible functionality.
 */
import { ref } from 'vue';

defineProps<{
  title: string;
  icon?: string;
  collapsible?: boolean;
  defaultCollapsed?: boolean;
}>();

const isCollapsed = ref(false);
</script>

<template>
  <div class="plugin-panel border border-border rounded-lg overflow-hidden bg-surface">
    <!-- Header -->
    <div
      class="plugin-panel-header flex items-center justify-between px-4 py-3 bg-surface-alt"
      :class="{ 'cursor-pointer hover:bg-hover': collapsible }"
      @click="collapsible && (isCollapsed = !isCollapsed)"
    >
      <div class="flex items-center gap-2">
        <slot name="icon">
          <svg v-if="icon" class="w-5 h-5 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" :d="icon" />
          </svg>
        </slot>
        <h3 class="font-medium text-primary">{{ title }}</h3>
      </div>

      <button
        v-if="collapsible"
        class="p-1 rounded hover:bg-hover transition-colors"
        :aria-label="isCollapsed ? 'Expand' : 'Collapse'"
      >
        <svg
          class="w-4 h-4 text-tertiary transition-transform"
          :class="{ 'rotate-180': !isCollapsed }"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
        </svg>
      </button>
    </div>

    <!-- Content -->
    <div v-show="!isCollapsed" class="plugin-panel-content p-4">
      <slot></slot>
    </div>
  </div>
</template>

<style scoped>
.plugin-panel {
  transition: border-color 0.2s;
}

.plugin-panel:hover {
  border-color: var(--border-hover);
}
</style>
