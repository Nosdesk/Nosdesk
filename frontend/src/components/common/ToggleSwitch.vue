<!--
On/off switch on Reka's Switch: `role=switch` with `aria-checked`, Space
and Enter toggle, `data-state` for styling, and a hidden checkbox for
forms when `name` is given. The label is a real `<label for>` on the
switch (one click toggles once) and the description is wired through
`aria-describedby`, so the control reads as "Label, description, switch,
on" instead of a bare button.
-->
<script setup lang="ts">
import { computed, useId } from 'vue'
import { SwitchRoot, SwitchThumb } from 'reka-ui'

interface Props {
  modelValue: boolean
  disabled?: boolean
  size?: 'sm' | 'md' | 'lg'
  label?: string
  description?: string
  /** Form field name; renders a hidden checkbox alongside. */
  name?: string
  id?: string
}

const props = withDefaults(defineProps<Props>(), {
  disabled: false,
  size: 'md',
})

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

const generatedId = useId()
const switchId = computed(() => props.id ?? `switch-${generatedId}`)
const descriptionId = computed(() => (props.description ? `${switchId.value}-desc` : undefined))

const sizeClasses = {
  sm: { track: 'h-5 w-9', thumb: 'h-4 w-4 data-[state=checked]:translate-x-4' },
  md: { track: 'h-6 w-11', thumb: 'h-5 w-5 data-[state=checked]:translate-x-5' },
  lg: { track: 'h-7 w-14', thumb: 'h-6 w-6 data-[state=checked]:translate-x-7' },
}

const currentSize = computed(() => sizeClasses[props.size])
</script>

<template>
  <div class="flex items-center justify-between gap-4" :class="label ? 'py-2' : ''">
    <div v-if="label || description" class="flex-1 min-w-0">
      <label
        v-if="label"
        :for="switchId"
        class="text-sm font-medium text-primary"
        :class="disabled ? 'cursor-not-allowed' : 'cursor-pointer'"
      >
        {{ label }}
      </label>
      <p v-if="description" :id="descriptionId" class="text-xs text-tertiary mt-0.5">
        {{ description }}
      </p>
    </div>

    <SwitchRoot
      :id="switchId"
      :model-value="modelValue"
      :disabled="disabled"
      :name="name"
      :aria-describedby="descriptionId"
      :class="[
        'relative inline-flex flex-shrink-0 items-center rounded-full border-2 transition-colors duration-200 ease-in-out',
        'focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface',
        'bg-surface-alt border-strong data-[state=checked]:bg-accent data-[state=checked]:border-accent',
        currentSize.track,
        disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer hover:opacity-90',
      ]"
      @update:model-value="emit('update:modelValue', $event)"
    >
      <SwitchThumb
        :class="[
          'toggle-thumb pointer-events-none inline-block rounded-full bg-white shadow-lg transition-transform duration-200 ease-in-out translate-x-0',
          currentSize.thumb,
        ]"
      />
    </SwitchRoot>
  </div>
</template>
