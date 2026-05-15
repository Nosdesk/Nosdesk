<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useMobileSearch } from '@/composables/useMobileSearch'
import Icon from '@/components/common/Icon.vue'

const {
  searchQuery,
  placeholder,
  isActive,
  handleSearchUpdate
} = useMobileSearch()

const fluent = useFluent()
const resolvedPlaceholder = computed(() => placeholder.value || fluent.$t('common-search-placeholder'))

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
        <div class="absolute inset-y-0 left-0 flex items-center pl-3 pointer-events-none text-tertiary">
          <Icon name="search" />
        </div>
        <input
          type="text"
          :value="localValue"
          @input="handleInput"
          :placeholder="resolvedPlaceholder"
          class="block w-full pl-9 pr-3 py-2 text-sm bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:ring-2 focus:ring-accent focus:border-accent"
        />
      </div>
    </div>
  </div>
</template>
