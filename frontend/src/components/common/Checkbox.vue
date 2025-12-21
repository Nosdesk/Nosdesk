<script setup lang="ts">
import { computed } from 'vue'

interface Props {
  modelValue: boolean
  disabled?: boolean
  indeterminate?: boolean
  size?: 'sm' | 'md' | 'lg'
  label?: string
  id?: string
}

const props = withDefaults(defineProps<Props>(), {
  disabled: false,
  indeterminate: false,
  size: 'md'
})

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  'change': [event: Event]
}>()

const toggle = (event: Event) => {
  if (!props.disabled) {
    emit('update:modelValue', !props.modelValue)
    emit('change', event)
  }
}

const sizeClasses = {
  sm: 'w-3.5 h-3.5',
  md: 'w-4 h-4',
  lg: 'w-5 h-5'
}

const currentSize = computed(() => sizeClasses[props.size])
</script>

<template>
  <div class="inline-flex items-center" :class="label ? 'gap-2' : ''">
    <button
      type="button"
      role="checkbox"
      :aria-checked="indeterminate ? 'mixed' : modelValue"
      :aria-label="label"
      :disabled="disabled"
      :id="id"
      @click="toggle"
      :class="[
        'relative inline-flex items-center justify-center rounded border-2 transition-colors duration-150 ease-in-out',
        'focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface',
        currentSize,
        modelValue || indeterminate
          ? 'bg-accent border-accent'
          : 'bg-surface border-strong',
        disabled
          ? 'opacity-50 cursor-not-allowed'
          : 'cursor-pointer hover:border-accent'
      ]"
    >
      <!-- Checkmark icon -->
      <svg
        v-if="modelValue && !indeterminate"
        class="w-full h-full text-white p-0.5"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        stroke-width="3"
      >
        <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
      </svg>
      <!-- Indeterminate icon -->
      <svg
        v-else-if="indeterminate"
        class="w-full h-full text-white p-0.5"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        stroke-width="3"
      >
        <path stroke-linecap="round" stroke-linejoin="round" d="M5 12h14" />
      </svg>
    </button>
    <label
      v-if="label"
      :for="id"
      class="text-sm text-primary cursor-pointer select-none"
      :class="disabled ? 'opacity-50 cursor-not-allowed' : ''"
      @click="toggle"
    >
      {{ label }}
    </label>
  </div>
</template>
