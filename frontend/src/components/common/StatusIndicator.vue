<script setup lang="ts">
/**
 * StatusIndicator Component
 *
 * Displays ticket status as a visual indicator.
 * Supports color blind friendly mode which uses shapes instead of colors:
 * - Open: hollow circle with outline
 * - In Progress: circle within outlined hollow circle (bullseye)
 * - Closed: filled solid circle
 */
import { computed } from 'vue'
import { useThemeStore } from '@/stores/theme'

type TicketStatus = 'open' | 'in-progress' | 'closed'

const props = withDefaults(defineProps<{
  status: TicketStatus
  size?: 'xs' | 'sm' | 'md'
}>(), {
  size: 'sm'
})

const themeStore = useThemeStore()

// Size classes for the indicator - larger when in color blind mode for better visibility
const sizeClasses = computed(() => {
  if (themeStore.effectiveColorBlindMode) {
    // Larger sizes for color blind mode to make shapes more visible
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
  // Standard mode sizes
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

// Color classes for standard mode
const colorClasses = computed(() => {
  switch (props.status) {
    case 'open':
      return 'bg-status-open'
    case 'in-progress':
      return 'bg-status-in-progress'
    case 'closed':
      return 'bg-status-closed'
    default:
      return 'bg-status-open'
  }
})

// Status labels for accessibility
const statusLabel = computed(() => {
  switch (props.status) {
    case 'open':
      return 'Open'
    case 'in-progress':
      return 'In Progress'
    case 'closed':
      return 'Closed'
    default:
      return 'Unknown'
  }
})
</script>

<template>
  <!-- Color blind friendly mode: use shapes -->
  <span
    v-if="themeStore.effectiveColorBlindMode"
    class="inline-flex items-center justify-center flex-shrink-0"
    :class="sizeClasses"
    :title="statusLabel"
    :aria-label="statusLabel"
    role="img"
  >
    <!-- Open: hollow circle (ring only) -->
    <svg
      v-if="status === 'open'"
      viewBox="0 0 10 10"
      class="w-full h-full"
      fill="none"
    >
      <circle
        cx="5"
        cy="5"
        r="4"
        stroke="currentColor"
        stroke-width="1.5"
        class="text-status-open"
      />
    </svg>

    <!-- In Progress: bullseye (ring with center dot) -->
    <svg
      v-else-if="status === 'in-progress'"
      viewBox="0 0 10 10"
      class="w-full h-full"
    >
      <circle
        cx="5"
        cy="5"
        r="4"
        stroke="currentColor"
        stroke-width="1.5"
        fill="none"
        class="text-status-in-progress"
      />
      <circle
        cx="5"
        cy="5"
        r="2"
        fill="currentColor"
        class="text-status-in-progress"
      />
    </svg>

    <!-- Closed: filled circle -->
    <svg
      v-else
      viewBox="0 0 10 10"
      class="w-full h-full"
    >
      <circle
        cx="5"
        cy="5"
        r="4"
        fill="currentColor"
        class="text-status-closed"
      />
    </svg>
  </span>

  <!-- Standard mode: colored dots -->
  <span
    v-else
    class="rounded-full flex-shrink-0"
    :class="[sizeClasses, colorClasses]"
    :title="statusLabel"
    :aria-label="statusLabel"
    role="img"
  ></span>
</template>
