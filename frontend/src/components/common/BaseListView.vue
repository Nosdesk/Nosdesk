<script setup lang="ts">
import { computed, ref, onMounted, onBeforeUnmount, shallowRef } from 'vue'

// Debounce utility for resize events
function debounce<T extends (...args: any[]) => void>(fn: T, delay: number): T {
  let timeoutId: ReturnType<typeof setTimeout> | null = null
  return ((...args: Parameters<T>) => {
    if (timeoutId) clearTimeout(timeoutId)
    timeoutId = setTimeout(() => fn(...args), delay)
  }) as T
}

const props = defineProps<{
  title: string
  isLoading: boolean
  isEmpty: boolean
  emptyMessage?: string
  error?: string | null
  // Results count
  resultsCount?: number
  // Sorting props
  sortField?: string
  sortDirection?: 'asc' | 'desc'
  // Column definitions for the table
  columns?: Array<{
    field: string
    label: string
    sortable?: boolean
    width?: string
    class?: string
  }>
  // Selection props
  selectedItems?: string[]
  itemIdField?: string
  enableSelection?: boolean
  visibleItems?: any[]

  // Legacy props (kept for backward compatibility but not used)
  searchQuery?: string
  showAddButton?: boolean
  addButtonText?: string
  filters?: Array<{
    name: string
    value: string
    options: Array<{ value: string, label: string }>
    width?: string
  }>
}>()

const emit = defineEmits<{
  'update:sort': [field: string, direction: 'asc' | 'desc']
  'update:selectedItems': [selectedIds: string[]]
  'toggle-selection': [event: Event, itemId: string]
  'toggle-all': [event: Event, checked: boolean]
  'import': []
  'retry': []
  // Legacy emits (kept for backward compatibility but not used)
  'update:searchQuery': [value: string]
  'update:filter': [name: string, value: string]
  'add': []
  'reset-filters': []
}>()

const defaultEmptyMessage = computed(() => {
  return `No ${props.title.toLowerCase()} found.`
})

// Handle sort toggle
const toggleSort = (field: string) => {
  if (props.sortField === field) {
    // Toggle direction if clicking the same field
    const newDirection = props.sortDirection === 'asc' ? 'desc' : 'asc'
    emit('update:sort', field, newDirection)
  } else {
    // Set new field and default to ascending
    emit('update:sort', field, 'asc')
  }
}

// Handle selection toggle for a single item
const toggleSelection = (event: Event, itemId: string) => {
  // We need to pass the original event to the parent component
  // so it can handle shift/ctrl/cmd key modifiers
  emit('toggle-selection', event, itemId)
}

// Handle toggle all items
const toggleAllItems = (event: Event) => {
  const checkbox = event.target as HTMLInputElement
  emit('toggle-all', event, checkbox.checked)
}

// Compute if all visible items are selected
const allSelected = computed(() => {
  if (!props.selectedItems || !props.visibleItems || props.visibleItems.length === 0) {
    return false
  }
  
  const idField = props.itemIdField || 'id'
  return props.visibleItems.every(item => 
    props.selectedItems?.includes(item[idField].toString())
  )
})



// Add isMobile ref to track screen size
const isMobile = ref(false)

// Function to check screen size
const checkScreenSize = () => {
  isMobile.value = window.innerWidth < 1024 // lg breakpoint - use card view for tablets too
}

// Debounced version - only updates after resize stops for 150ms
// This prevents 60+ re-renders per second during active resizing
const debouncedCheckScreenSize = debounce(checkScreenSize, 150)

// Initialize on mount
onMounted(() => {
  checkScreenSize() // Initial check (immediate)
  window.addEventListener('resize', debouncedCheckScreenSize)
})

// Clean up on unmount
onBeforeUnmount(() => {
  window.removeEventListener('resize', debouncedCheckScreenSize)
})
</script>

<template>
  <div class="flex flex-col flex-1 min-h-0">
    <!-- Optional header slot for custom content -->
    <slot name="header-extra"></slot>

    <!-- Main content container -->
    <div class="flex-1 flex flex-col overflow-hidden">
      <!-- Loading state -->
      <div v-if="isLoading" class="flex justify-center items-center h-64">
        <div class="animate-spin rounded-full h-12 w-12 border-t-2 border-b-2 border-brand-blue"></div>
      </div>

      <!-- Error state -->
      <div v-else-if="error" class="flex flex-col items-center gap-4 justify-center p-8 text-center text-status-error">
        {{ error }}
        <button @click="emit('retry')" class="mt-4 px-4 py-2 bg-brand-blue text-white rounded hover:opacity-90 transition-colors">
          Try Again
        </button>
      </div>

      <!-- Empty state -->
      <div v-else-if="isEmpty" class="p-8 text-center text-secondary">
        {{ emptyMessage || defaultEmptyMessage }}
      </div>

      <!-- Content -->
      <div v-else class="flex-1 flex flex-col overflow-hidden">
        <!-- Desktop Table View - hidden on mobile via CSS -->
        <div v-show="!isMobile" class="contents">
          <!-- Table Header (if columns are provided) -->
          <div v-if="columns && columns.length > 0" class="sticky top-0 z-10 bg-surface border-b border-default text-sm text-primary">
            <div class="flex min-w-[800px] gap-1">
              <!-- Selection checkbox in header -->
              <div v-if="enableSelection" class="p-3 py-4 w-10 flex-shrink-0">
                <input
                  type="checkbox"
                  class="w-4 h-4 rounded border-default bg-surface-alt text-brand-blue focus:ring-brand-blue"
                  :checked="allSelected && visibleItems && visibleItems.length > 0"
                  :indeterminate="selectedItems && selectedItems.length > 0 && !allSelected"
                  @change="toggleAllItems"
                />
              </div>

              <!-- Custom header prefix slot -->
              <slot name="header-prefix" class="p-3 py-4"></slot>

              <!-- Generated column headers -->
              <div
                v-for="column in columns"
                :key="column.field"
                :class="[
                  'p-3 py-4 font-medium',
                  column.sortable !== false ? 'cursor-pointer select-none' : '',
                  column.width || (column.field === 'id' ? 'w-20 flex-shrink-0' : 'flex-1 min-w-0'),
                  column.class || ''
                ]"
                @click="column.sortable !== false && toggleSort(column.field)"
              >
                <div class="flex items-center gap-1">
                  {{ column.label }}
                  <span v-if="column.sortable !== false && sortField === column.field" class="text-primary">
                    {{ sortDirection === 'asc' ? '↑' : '↓' }}
                  </span>
                </div>
              </div>

              <!-- Slot for additional header columns -->
              <slot name="header-suffix" class="p-3 py-4"></slot>
            </div>
          </div>

          <!-- Table Body -->
          <div class="flex-1 overflow-y-auto">
            <slot></slot>
          </div>
        </div>

        <!-- Mobile Card View - hidden on desktop via CSS -->
        <div v-show="isMobile" class="flex-1 overflow-y-auto p-2 flex flex-col gap-2">
          <slot name="mobile-view"></slot>
        </div>


      </div>
    </div>
  </div>
</template>

<style scoped>
/* Ensure consistent layout */
.flex-1 {
  flex: 1 1 0%;
}

/* Prevent horizontal scrolling unless necessary */
body {
  overflow-x: hidden;
}

/* Custom scrollbar styling */
.overflow-y-auto::-webkit-scrollbar {
  width: 8px;
}

.overflow-y-auto::-webkit-scrollbar-track {
  background: var(--color-bg-surface);
}

.overflow-y-auto::-webkit-scrollbar-thumb {
  background: var(--color-border-default);
  border-radius: 4px;
}

.overflow-y-auto::-webkit-scrollbar-thumb:hover {
  background: var(--color-border-strong);
}

/* Mobile-specific styles */
@media (max-width: 767px) {
  .flex-1 {
    min-height: 0;
  }
  
  /* Make buttons more touch-friendly */
  button {
    padding: 0.5rem 1rem;
    min-width: 44px;
  }
  
  /* Adjust spacing for mobile */

}
</style> 