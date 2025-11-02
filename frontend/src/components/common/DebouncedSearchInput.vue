<template>
  <div class="relative flex-grow min-w-[150px]">
    <div class="absolute inset-y-0 left-0 flex items-center pl-2 pointer-events-none">
      <svg
        class="w-3.5 h-3.5 text-tertiary"
        aria-hidden="true"
        xmlns="http://www.w3.org/2000/svg"
        fill="none"
        viewBox="0 0 20 20"
      >
        <path
          stroke="currentColor"
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
          d="m19 19-4-4m0-7A7 7 0 1 1 1 8a7 7 0 0 1 14 0Z"
        />
      </svg>
    </div>
    <input
      ref="inputRef"
      type="text"
      v-model="localValue"
      @input="handleInput"
      class="block w-full py-1 pl-8 pr-2 text-sm border rounded-md bg-surface border-default placeholder-tertiary text-primary focus:ring-blue-500 focus:border-blue-500"
      :placeholder="placeholder"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'

interface Props {
  modelValue: string
  placeholder?: string
  debounceMs?: number
}

interface Emits {
  (e: 'update:modelValue', value: string): void
}

const props = withDefaults(defineProps<Props>(), {
  placeholder: 'Search...',
  debounceMs: 300
})

const emit = defineEmits<Emits>()

// Local state - completely isolated from parent
const localValue = ref(props.modelValue)
const inputRef = ref<HTMLInputElement>()
let timeoutId: number | null = null

// Watch for external changes (like reset button)
watch(() => props.modelValue, (newValue) => {
  if (newValue !== localValue.value) {
    localValue.value = newValue
  }
})

const handleInput = () => {
  // Clear existing timeout
  if (timeoutId) {
    clearTimeout(timeoutId)
  }
  
  // Set new timeout
  timeoutId = setTimeout(() => {
    emit('update:modelValue', localValue.value)
  }, props.debounceMs)
}
</script> 