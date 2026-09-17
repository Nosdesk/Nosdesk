<!--
Checkbox on Reka's Checkbox: `role=checkbox` with `aria-checked` (`mixed`
when indeterminate), Space toggles, Enter is swallowed, `data-state` for
styling and a hidden input for forms when `name` is given. A click on an
indeterminate box lands on checked.

`change` carries the native click event alongside `update:modelValue`;
row-selection consumers read `shiftKey` off it for range select.
-->
<script setup lang="ts">
import { computed, useId } from 'vue'
import { CheckboxIndicator, CheckboxRoot } from 'reka-ui'
import { useColorFilter } from '@/composables/useColorFilter'

const { colorFilterStyle } = useColorFilter()

interface Props {
  modelValue: boolean
  disabled?: boolean
  indeterminate?: boolean
  size?: 'sm' | 'md' | 'lg'
  label?: string
  /** Screen-reader-only label. Use this when the checkbox is
   *  presented in a context that already carries the meaning
   *  visually (e.g. a row checkbox in a data table) so the
   *  visible `label` slot doesn't render duplicate text but
   *  assistive tech still announces the right name. Falls back
   *  to `label` if omitted. */
  ariaLabel?: string
  id?: string
  /** Form field name; renders a hidden input alongside. */
  name?: string
}

const props = withDefaults(defineProps<Props>(), {
  disabled: false,
  indeterminate: false,
  size: 'md',
})

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  change: [event: Event]
}>()

const generatedId = useId()
const checkboxId = computed(() => props.id ?? `checkbox-${generatedId}`)

const state = computed<boolean | 'indeterminate'>(() =>
  props.indeterminate ? 'indeterminate' : props.modelValue,
)

function onUpdate(next: boolean | 'indeterminate') {
  emit('update:modelValue', next === true)
}

const sizeClasses = {
  sm: 'w-3.5 h-3.5',
  md: 'w-4 h-4',
  lg: 'w-5 h-5',
}

const currentSize = computed(() => sizeClasses[props.size])

// Apply color filter when checked (for Red Horizon theme)
const checkboxStyle = computed(() =>
  props.modelValue || props.indeterminate ? colorFilterStyle.value : {},
)
</script>

<template>
  <div class="inline-flex items-center" :class="label ? 'gap-2' : ''">
    <CheckboxRoot
      :id="checkboxId"
      :model-value="state"
      :disabled="disabled"
      :name="name"
      :aria-label="ariaLabel ?? label"
      :style="checkboxStyle"
      :class="[
        'relative inline-flex items-center justify-center rounded border-2 transition-colors duration-150 ease-in-out',
        'focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface',
        'bg-surface border-strong data-[state=checked]:bg-accent data-[state=checked]:border-accent',
        'data-[state=indeterminate]:bg-accent data-[state=indeterminate]:border-accent',
        currentSize,
        disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer hover:border-accent',
      ]"
      @update:model-value="onUpdate"
      @click="emit('change', $event)"
    >
      <CheckboxIndicator as-child>
        <svg
          class="w-full h-full text-white p-0.5"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          stroke-width="3"
          aria-hidden="true"
        >
          <path v-if="indeterminate" stroke-linecap="round" stroke-linejoin="round" d="M5 12h14" />
          <path v-else stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
        </svg>
      </CheckboxIndicator>
    </CheckboxRoot>
    <label
      v-if="label"
      :for="checkboxId"
      class="text-sm text-primary select-none"
      :class="disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'"
    >
      {{ label }}
    </label>
  </div>
</template>
