<!-- components/CustomDropdown.vue -->
<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'

const props = defineProps<{
  value: string
  options: { value: string; label: string }[]
  type: 'status' | 'priority'
}>()

const emit = defineEmits<{
  (e: 'update:value', value: string): void
}>()

const isOpen = ref(false)
const dropdownRef = ref<HTMLElement | null>(null)

const selectedOption = computed(() =>
  props.options.find(option => option.value === props.value)
)

// Computed position for the dropdown
const dropdownPosition = computed(() => {
  if (!dropdownRef.value || !isOpen.value) {
    return { top: '0px', left: '0px', width: '0px' }
  }
  
  const rect = dropdownRef.value.getBoundingClientRect()
  return {
    top: `${rect.bottom + (window as any).scrollY + 4}px`,
    left: `${rect.left + (window as any).scrollX}px`,
    width: `${rect.width}px`
  }
})

const toggleDropdown = () => {
  isOpen.value = !isOpen.value
}

const selectOption = (option: { value: string; label: string }) => {
  emit('update:value', option.value)
  isOpen.value = false
}

// Close dropdown when clicking outside
const handleClickOutside = (event: MouseEvent) => {
  if (dropdownRef.value && !dropdownRef.value.contains(event.target as Node)) {
    isOpen.value = false
  }
}

// Simple status indicator colors
const getStatusColor = (value: string) => {
  if (props.type === 'status') {
    switch (value) {
      case 'open':
        return 'bg-amber-500'
      case 'in-progress':
        return 'bg-blue-500'
      case 'closed':
        return 'bg-green-500'
      default:
        return 'bg-slate-500'
    }
  } else if (props.type === 'priority') {
    switch (value) {
      case 'low':
        return 'bg-green-500'
      case 'medium':
        return 'bg-amber-500'
      case 'high':
        return 'bg-red-500'
      default:
        return 'bg-slate-500'
    }
  }
  return 'bg-slate-500'
}

onMounted(() => {
  document.addEventListener('click', handleClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
})
</script>

<template>
  <div class="relative" ref="dropdownRef">
    <!-- Dropdown trigger -->
    <button
      type="button"
      @click="toggleDropdown"
      class="w-full px-3 py-2 bg-transparent text-primary text-left flex items-center justify-between hover:bg-surface-hover transition-colors rounded-lg"
    >
      <div class="flex items-center gap-2">
        <div
          class="w-2 h-2 rounded-full"
          :class="getStatusColor(value)"
        ></div>
        <span class="text-sm">{{ selectedOption?.label || 'Select an option' }}</span>
      </div>
      <svg
        class="w-4 h-4 text-tertiary transition-transform duration-200"
        :class="{ 'rotate-180': isOpen }"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
      </svg>
    </button>

    <!-- Dropdown menu -->
    <Teleport to="body">
      <div
        v-if="isOpen && dropdownRef"
        class="fixed bg-surface border border-default rounded-lg shadow-lg overflow-hidden min-w-max"
        :style="{
          ...dropdownPosition,
          zIndex: 9999
        }"
      >
        <div class="py-1">
          <button
            v-for="option in options"
            :key="option.value"
            @click="selectOption(option)"
            class="w-full px-3 py-2 text-left text-primary hover:bg-surface-hover transition-colors flex items-center gap-2"
            :class="{ 'bg-surface-hover': option.value === value }"
          >
            <div 
              class="w-2 h-2 rounded-full"
              :class="getStatusColor(option.value)"
            ></div>
            <span class="text-sm">{{ option.label }}</span>
          </button>
        </div>
      </div>
    </Teleport>
  </div>
</template>