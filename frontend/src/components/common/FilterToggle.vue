<!--
Icon-only toggle pill for boolean filter controls. Provides the
standard 24×24 pressed/unpressed treatment used beside dropdown-style
filters in the dashboard widget subheaders. Takes care of aria-pressed,
the tinted active background, and the context-aware tooltip ("X only"
when off, "Showing X — click to clear" when on).

Callers supply the icon via the default slot and a Tailwind class for
the active-state colour (e.g. `text-priority-high`, `text-accent`).
-->
<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  modelValue: boolean
  /** The control's name, used for aria-label and as the tooltip root.
   *  Rendered as-is, so keep it title-cased and human-readable. */
  label: string
  /** Tailwind classes applied to the button when the toggle is on.
   *  Typically a tinted bg + coloured text (e.g.
   *  `bg-priority-high/15 text-priority-high`). */
  activeClass: string
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
}>()

const tooltip = computed(() =>
  props.modelValue
    ? `Showing ${props.label.toLowerCase()}. Click to clear.`
    : props.label,
)

function toggle() {
  emit('update:modelValue', !props.modelValue)
}
</script>

<template>
  <button
    type="button"
    :aria-pressed="modelValue"
    :aria-label="label"
    :title="tooltip"
    :class="[
      'w-6 h-6 inline-flex items-center justify-center rounded-md transition-colors',
      modelValue ? activeClass : 'text-tertiary hover:text-primary hover:bg-surface/60',
    ]"
    @click="toggle"
  >
    <slot />
  </button>
</template>
