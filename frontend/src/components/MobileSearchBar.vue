<script setup lang="ts">
import { ref, watch } from 'vue'
import { useMobileSearch } from '@/composables/useMobileSearch'

const {
  searchQuery,
  placeholder,
  isActive,
  handleSearchUpdate
} = useMobileSearch()

const localValue = ref(searchQuery.value)
let debounceTimer: ReturnType<typeof setTimeout> | null = null

// Sync local value when global state changes
watch(searchQuery, (newVal) => {
  localValue.value = newVal
})

const handleInput = (event: Event) => {
  const value = (event.target as HTMLInputElement).value
  localValue.value = value

  if (debounceTimer) {
    clearTimeout(debounceTimer)
  }

  debounceTimer = setTimeout(() => {
    handleSearchUpdate(value)
  }, 300)
}
</script>

<template>
  <div
    v-if="isActive"
    class="fixed left-0 right-0 bg-surface border-t border-default z-20 sm:hidden print:hidden"
    style="bottom: calc(3rem + env(safe-area-inset-bottom))"
  >
    <div class="flex items-center gap-2 px-3 py-2">
      <!-- Search Input -->
      <div class="relative flex-1">
        <div class="absolute inset-y-0 left-0 flex items-center pl-3 pointer-events-none">
          <svg class="w-4 h-4 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
        </div>
        <input
          type="text"
          :value="localValue"
          @input="handleInput"
          :placeholder="placeholder"
          class="block w-full pl-9 pr-3 py-2 text-sm bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:ring-2 focus:ring-accent focus:border-accent"
        />
      </div>
    </div>
  </div>
</template>
