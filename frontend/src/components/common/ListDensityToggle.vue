<script setup lang="ts">
/**
 * Three-step row density control (compact / cosy / comfortable).
 * Shared by tickets and projects list toolbars. Always has a value, so
 * it is a radio group (Reka's RadioGroup: arrows move and select, the
 * group is one tab stop). Each icon-only item is named by its aria-label
 * and hinted by a Tooltip, which also reaches keyboard users.
 *
 * Styled on `aria-checked` rather than `data-state`: the tooltip trigger
 * writes its own `data-state` onto the same element.
 */
import { RadioGroupItem, RadioGroupRoot } from 'reka-ui'
import Tooltip from './Tooltip.vue'
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
  <RadioGroupRoot
    :model-value="density"
    :aria-label="$t('views-display-menu-density-aria')"
    orientation="horizontal"
    loop
    class="inline-flex items-center rounded-md border border-subtle overflow-hidden h-7"
    @update:model-value="emit('set-density', $event as Density)"
  >
    <Tooltip v-for="opt in densityOptions" :key="opt.value" :text="$t(opt.labelKey)">
      <RadioGroupItem :value="opt.value" as-child>
        <button
          type="button"
          class="h-full w-7 flex items-center justify-center transition-colors text-tertiary hover:text-primary hover:bg-surface-hover aria-checked:bg-accent/15 aria-checked:text-accent focus:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent"
          :aria-label="$t(opt.labelKey)"
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
      </RadioGroupItem>
    </Tooltip>
  </RadioGroupRoot>
</template>
