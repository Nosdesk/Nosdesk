<script setup lang="ts">
/**
 * Three-step row density control (compact / cosy / comfortable).
 * Shared by tickets and projects list toolbars.
 */
import type { Density } from '@/composables/useTicketsDensity'

defineProps<{
  density: Density
}>()

const emit = defineEmits<{
  'set-density': [value: Density]
}>()

const densityOptions: ReadonlyArray<{ value: Density; svg: string; labelKey: string }> = [
  {
    value: 'compact',
    labelKey: 'views-display-menu-density-compact',
    svg: 'M3 5h14M3 9h14M3 13h14M3 17h14',
  },
  {
    value: 'cosy',
    labelKey: 'views-display-menu-density-cosy',
    svg: 'M3 5h14M3 10h14M3 15h14',
  },
  {
    value: 'comfortable',
    labelKey: 'views-display-menu-density-comfortable',
    svg: 'M3 6h14M3 14h14',
  },
]
</script>

<template>
  <div
    class="inline-flex items-center rounded-md border border-subtle overflow-hidden h-7"
    role="group"
    :aria-label="$t('views-display-menu-density-aria')"
  >
    <button
      v-for="opt in densityOptions"
      :key="opt.value"
      type="button"
      class="h-full w-7 flex items-center justify-center transition-colors"
      :class="density === opt.value
        ? 'bg-accent/15 text-accent'
        : 'text-tertiary hover:text-primary hover:bg-surface-hover'"
      :aria-pressed="density === opt.value"
      :title="$t(opt.labelKey)"
      @click="emit('set-density', opt.value)"
    >
      <svg
        viewBox="0 0 20 20"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-linecap="round"
        class="w-3.5 h-3.5"
        aria-hidden="true"
      >
        <path :d="opt.svg" />
      </svg>
    </button>
  </div>
</template>
