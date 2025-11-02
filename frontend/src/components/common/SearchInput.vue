<script setup lang="ts">
import { ref, watch } from 'vue'

const props = defineProps<{
  placeholder?: string
  modelValue: string
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()

const localSearchQuery = ref(props.modelValue)

watch(() => props.modelValue, (newValue) => {
  localSearchQuery.value = newValue
})

watch(localSearchQuery, (newValue) => {
  emit('update:modelValue', newValue)
})
</script>

<template>
  <div class="relative">
    <input
      v-model="localSearchQuery"
      type="text"
      :placeholder="placeholder || 'Search...'"
      class="w-full md:w-64 bg-surface text-primary rounded-lg px-4 py-2 focus:outline-none focus:ring-2 focus:ring-blue-500"
    >
    <div class="absolute inset-y-0 right-0 flex items-center pr-3 pointer-events-none">
      <svg class="w-5 h-5 text-tertiary" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
        <path fill-rule="evenodd" d="M9 3.5a5.5 5.5 0 100 11 5.5 5.5 0 000-11zM2 9a7 7 0 1112.452 4.391l3.328 3.329a.75.75 0 11-1.06 1.06l-3.329-3.328A7 7 0 012 9z" clip-rule="evenodd" />
      </svg>
    </div>
  </div>
</template> 