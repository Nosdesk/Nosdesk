<script setup lang="ts">
import { ref } from 'vue'

const props = defineProps<{
  title: string
  isCollapsed: boolean
  // Icon type: 'clock' for recent items, 'book' for documentation
  icon?: 'clock' | 'book'
  // accentColor prop kept for backwards compatibility but no longer used
  accentColor?: string
}>()

const emit = defineEmits<{
  (e: 'toggle'): void
}>()

// Expose root element for parent components that need DOM access (e.g., resizable sections)
const rootRef = ref<HTMLElement | null>(null)
defineExpose({
  $el: rootRef
})
</script>

<template>
  <div
    ref="rootRef"
    class="flex flex-col overflow-hidden transition-opacity duration-200"
    :class="[isCollapsed ? 'opacity-90 hover:opacity-100' : '']"
  >
    <!-- Header -->
    <div
      class="flex items-center justify-between py-1.5 px-3 cursor-pointer transition-colors duration-200 group flex-shrink-0"
      :class="isCollapsed ? 'hover:bg-surface-hover' : 'bg-surface-hover/40'"
      @click="emit('toggle')"
    >
      <h3 class="text-xs font-medium text-secondary uppercase tracking-wider flex items-center gap-1.5">
        <!-- Clock icon for recent tickets -->
        <svg v-if="icon === 'clock'" class="w-3 h-3 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <!-- Book icon for documentation -->
        <svg v-else-if="icon === 'book'" class="w-3 h-3 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
        </svg>
        <!-- Default dot fallback -->
        <span v-else class="w-2 h-2 rounded-full bg-accent"></span>
        {{ title }}
      </h3>
      <button
        class="text-tertiary group-hover:text-primary transition-colors duration-200 bg-surface-hover rounded p-0.5"
        :title="isCollapsed ? 'Expand section' : 'Collapse section'"
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          class="h-3 w-3 transition-transform duration-200"
          :class="{ 'rotate-180': !isCollapsed }"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M19 9l-7 7-7-7"
          />
        </svg>
      </button>
    </div>

    <!-- Content wrapper - parent controls height/flex behavior -->
    <div
      class="overflow-y-auto bg-surface/60 transition-opacity duration-200"
      :class="isCollapsed ? 'opacity-0 h-0' : 'opacity-100 flex-1'"
    >
      <slot v-if="!isCollapsed" />
    </div>
  </div>
</template>
