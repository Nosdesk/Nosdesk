<script setup lang="ts">
import { computed } from 'vue'

interface Props {
  modelValue: boolean
  disabled?: boolean
  size?: 'sm' | 'md' | 'lg'
  label?: string
  description?: string
}

const props = withDefaults(defineProps<Props>(), {
  disabled: false,
  size: 'md'
})

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

const toggle = () => {
  if (!props.disabled) {
    emit('update:modelValue', !props.modelValue)
  }
}

const sizeClasses = {
  sm: {
    container: 'h-5 w-9',
    thumb: 'h-4 w-4',
    translateOn: 'translate-x-4',
    translateOff: 'translate-x-0'
  },
  md: {
    container: 'h-6 w-11',
    thumb: 'h-5 w-5',
    translateOn: 'translate-x-5',
    translateOff: 'translate-x-0'
  },
  lg: {
    container: 'h-7 w-14',
    thumb: 'h-6 w-6',
    translateOn: 'translate-x-7',
    translateOff: 'translate-x-0'
  }
}

const currentSize = computed(() => sizeClasses[props.size])
</script>

<template>
  <div class="flex items-center justify-between gap-4" :class="label ? 'py-2' : ''">
    <div v-if="label || description" class="flex-1 min-w-0">
      <label v-if="label" class="text-sm font-medium text-primary cursor-pointer" @click="toggle">
        {{ label }}
      </label>
      <p v-if="description" class="text-xs text-tertiary mt-0.5">{{ description }}</p>
    </div>

    <button
      type="button"
      role="switch"
      :aria-checked="modelValue"
      :aria-label="label"
      :disabled="disabled"
      @click="toggle"
      :class="[
        'relative inline-flex flex-shrink-0 items-center rounded-full border-2 transition-colors duration-200 ease-in-out',
        'focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface',
        currentSize.container,
        modelValue
          ? 'bg-accent border-accent'
          : 'bg-surface-alt border-strong',
        disabled
          ? 'opacity-50 cursor-not-allowed'
          : 'cursor-pointer hover:opacity-90'
      ]"
    >
      <span
        aria-hidden="true"
        :class="[
          'toggle-thumb pointer-events-none inline-block rounded-full shadow-lg transition-transform duration-200 ease-in-out',
          currentSize.thumb,
          'bg-white',
          modelValue ? currentSize.translateOn : currentSize.translateOff
        ]"
      />
    </button>
  </div>
</template>
