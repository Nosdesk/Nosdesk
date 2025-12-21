<script setup lang="ts">
/**
 * PriorityIndicator Component
 *
 * Displays ticket priority as a visual indicator.
 * - Standard mode: Simple filled circles with priority colors
 * - Color blind mode: Rounded squares with distinct fill levels
 *   - Low: empty rounded square (outline only)
 *   - Medium: half-filled rounded square
 *   - High: fully filled rounded square
 */
import { computed } from 'vue'
import { useThemeStore } from '@/stores/theme'

type TicketPriority = 'low' | 'medium' | 'high'

const props = withDefaults(defineProps<{
  priority: TicketPriority
  size?: 'xs' | 'sm' | 'md'
}>(), {
  size: 'sm'
})

const themeStore = useThemeStore()

// Generate unique ID for clip path to avoid conflicts with multiple instances
const clipPathId = `halfFill-${Math.random().toString(36).substr(2, 9)}`

// Size classes for the indicator - larger when in color blind mode
const sizeClasses = computed(() => {
  if (themeStore.colorBlindMode) {
    switch (props.size) {
      case 'xs':
        return 'w-2.5 h-2.5'
      case 'sm':
        return 'w-3 h-3'
      case 'md':
        return 'w-3.5 h-3.5'
      default:
        return 'w-3 h-3'
    }
  }
  // Standard mode sizes (simple circles)
  switch (props.size) {
    case 'xs':
      return 'w-1.5 h-1.5'
    case 'sm':
      return 'w-2 h-2'
    case 'md':
      return 'w-2.5 h-2.5'
    default:
      return 'w-2 h-2'
  }
})

// Color classes for standard mode (filled circles)
const colorClasses = computed(() => {
  switch (props.priority) {
    case 'low':
      return 'bg-priority-low'
    case 'medium':
      return 'bg-priority-medium'
    case 'high':
      return 'bg-priority-high'
    default:
      return 'bg-tertiary'
  }
})

// Priority labels for accessibility
const priorityLabel = computed(() => {
  switch (props.priority) {
    case 'low':
      return 'Low Priority'
    case 'medium':
      return 'Medium Priority'
    case 'high':
      return 'High Priority'
    default:
      return 'Unknown Priority'
  }
})
</script>

<template>
  <!-- Color blind mode: distinct shapes -->
  <span
    v-if="themeStore.colorBlindMode"
    class="inline-flex items-center justify-center flex-shrink-0"
    :class="sizeClasses"
    :title="priorityLabel"
    :aria-label="priorityLabel"
    role="img"
  >
    <!-- Low: empty rounded square (outline only) -->
    <svg
      v-if="priority === 'low'"
      viewBox="0 0 10 10"
      class="w-full h-full"
      fill="none"
    >
      <rect
        x="1"
        y="1"
        width="8"
        height="8"
        rx="2"
        stroke="currentColor"
        stroke-width="1.5"
        class="text-priority-low"
      />
    </svg>

    <!-- Medium: half-filled rounded square -->
    <svg
      v-else-if="priority === 'medium'"
      viewBox="0 0 10 10"
      class="w-full h-full"
    >
      <!-- Clip path for bottom half fill -->
      <defs>
        <clipPath :id="clipPathId">
          <rect x="0" y="5" width="10" height="5" />
        </clipPath>
      </defs>
      <!-- Outline -->
      <rect
        x="1"
        y="1"
        width="8"
        height="8"
        rx="2"
        stroke="currentColor"
        stroke-width="1.5"
        fill="none"
        class="text-priority-medium"
      />
      <!-- Bottom half fill -->
      <rect
        x="1"
        y="1"
        width="8"
        height="8"
        rx="2"
        fill="currentColor"
        :clip-path="`url(#${clipPathId})`"
        class="text-priority-medium"
      />
    </svg>

    <!-- High: fully filled rounded square -->
    <svg
      v-else
      viewBox="0 0 10 10"
      class="w-full h-full"
    >
      <rect
        x="1"
        y="1"
        width="8"
        height="8"
        rx="2"
        fill="currentColor"
        class="text-priority-high"
      />
    </svg>
  </span>

  <!-- Standard mode: simple filled circles -->
  <span
    v-else
    class="rounded-full flex-shrink-0"
    :class="[sizeClasses, colorClasses]"
    :title="priorityLabel"
    :aria-label="priorityLabel"
    role="img"
  ></span>
</template>
