<!--
Icon-only toggle pill for boolean filter controls: the standard 24x24
pressed/unpressed treatment beside dropdown-style filters in the
dashboard widget subheaders. On Reka's Toggle (`aria-pressed`, Space and
Enter toggle), with a Tooltip carrying the context-aware hint (the label
when off, "label. Click to clear." when on) that keyboard users reach too.

Callers supply the icon via the default slot and a Tailwind class for
the active-state colour (e.g. `text-priority-high`, `text-accent`).
Styled on `aria-pressed`, since the tooltip trigger owns `data-state`.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { Toggle } from 'reka-ui'
import { useFluent } from 'fluent-vue'
import Tooltip from './Tooltip.vue'

const props = defineProps<{
  modelValue: boolean
  /** The control's name, used for aria-label and as the tooltip root. */
  label: string
  /** Tailwind classes applied to the button when the toggle is on.
   *  Typically a tinted bg + coloured text (e.g.
   *  `bg-priority-high/15 text-priority-high`). */
  activeClass: string
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
}>()

const fluent = useFluent()
const hint = computed(() =>
  props.modelValue ? fluent.$t('filter-toggle-clear-hint', { label: props.label }) : props.label,
)
</script>

<template>
  <Tooltip :text="hint">
    <Toggle
      :model-value="modelValue"
      :aria-label="label"
      :class="[
        'w-6 h-6 inline-flex items-center justify-center rounded-md transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-accent',
        modelValue ? activeClass : 'text-tertiary hover:text-primary hover:bg-surface/60',
      ]"
      @update:model-value="emit('update:modelValue', $event)"
    >
      <slot />
    </Toggle>
  </Tooltip>
</template>
