<script setup lang="ts">
import { ref, watch } from 'vue'
import { useMobileSearch } from '@/composables/useMobileSearch'

const {
  searchQuery,
  placeholder,
  showCreateButton,
  createButtonLoading,
  isActive,
  handleSearchUpdate,
  handleCreate
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

      <!-- Create Button -->
      <button
        v-if="showCreateButton"
        @click="handleCreate"
        :disabled="createButtonLoading"
        class="flex-shrink-0 p-2.5 bg-accent text-white rounded-lg hover:bg-accent-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed min-h-[44px] min-w-[44px] flex items-center justify-center"
        aria-label="Create"
      >
        <svg
          v-if="createButtonLoading"
          class="w-5 h-5 animate-spin"
          fill="none"
          viewBox="0 0 24 24"
        >
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
          <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
        </svg>
        <svg
          v-else
          class="w-5 h-5"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
        </svg>
      </button>
    </div>
  </div>
</template>
