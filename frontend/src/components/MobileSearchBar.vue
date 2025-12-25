<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import { useMobileSearch, type CreateButtonIcon } from '@/composables/useMobileSearch'

const {
  searchQuery,
  placeholder,
  showCreateButton,
  createButtonLoading,
  createButtonIcon,
  isActive,
  handleSearchUpdate,
  handleCreate
} = useMobileSearch()

// Icon SVG paths for each type - all include a small plus indicator
const iconPaths: Record<CreateButtonIcon, { main: string; viewBox?: string }> = {
  plus: {
    main: 'M12 6v6m0 0v6m0-6h6m-6 0H6'
  },
  ticket: {
    main: 'M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2'
  },
  user: {
    main: 'M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z'
  },
  device: {
    main: 'M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z'
  },
  folder: {
    main: 'M4 4h4v16H4V4zm6 0h4v12h-4V4zm6 0h4v8h-4V4z'
  },
  document: {
    main: 'M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z'
  }
}

const currentIcon = computed(() => iconPaths[createButtonIcon.value] || iconPaths.plus)

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
      <!-- Create Button - on left to avoid confusion with search submit -->
      <button
        v-if="showCreateButton"
        @click="handleCreate"
        :disabled="createButtonLoading"
        class="flex-shrink-0 p-2.5 bg-accent text-white rounded-lg hover:bg-accent-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed min-h-[44px] min-w-[44px] flex items-center justify-center relative"
        aria-label="Create"
      >
        <!-- Loading spinner -->
        <svg
          v-if="createButtonLoading"
          class="w-5 h-5 animate-spin"
          fill="none"
          viewBox="0 0 24 24"
        >
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
          <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
        </svg>
        <!-- Context-specific icon -->
        <template v-else>
          <!-- Main icon -->
          <svg
            class="w-5 h-5"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" :d="currentIcon.main" />
          </svg>
          <!-- Plus badge (only for non-plus icons) -->
          <span
            v-if="createButtonIcon !== 'plus'"
            class="absolute -top-0.5 -right-0.5 w-3.5 h-3.5 bg-surface text-accent rounded-full flex items-center justify-center shadow-sm border border-default"
          >
            <svg class="w-2.5 h-2.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="3">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v12m6-6H6" />
            </svg>
          </span>
        </template>
      </button>

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
