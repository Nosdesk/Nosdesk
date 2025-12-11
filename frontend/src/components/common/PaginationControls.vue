<script setup lang="ts">
import { computed, ref, onMounted, onBeforeUnmount, watch } from 'vue'

// Debounce utility for resize events
function debounce<T extends (...args: any[]) => void>(fn: T, delay: number): T {
  let timeoutId: ReturnType<typeof setTimeout> | null = null
  return ((...args: Parameters<T>) => {
    if (timeoutId) clearTimeout(timeoutId)
    timeoutId = setTimeout(() => fn(...args), delay)
  }) as T
}

const props = defineProps<{
  currentPage?: number
  totalPages?: number
  pageSize?: number
  pageSizeOptions?: number[]
  showImport?: boolean
}>()

const emit = defineEmits<{
  'update:currentPage': [page: number]
  'update:pageSize': [size: number]
  'import': []
}>()

// Add isMobile ref to track screen size
const isMobile = ref(false)
const pageInputValue = ref('')
const pageInput = ref<HTMLInputElement | null>(null)

// Function to check screen size
const checkScreenSize = () => {
  isMobile.value = window.innerWidth < 768 // md breakpoint
}

// Debounced version - only updates after resize stops for 150ms
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

// Watch for currentPage changes to update input value
watch(() => props.currentPage, (newPage) => {
  if (newPage) {
    pageInputValue.value = newPage.toString()
  }
}, { immediate: true })

// Pagination methods
const changePage = (page: number) => {
  if (page >= 1 && page <= (props.totalPages || 1)) {
    emit('update:currentPage', page)
  }
}

const changePageSize = (event: Event) => {
  const select = event.target as HTMLSelectElement
  emit('update:pageSize', parseInt(select.value))
}

// Handle direct page input
const handlePageInput = () => {
  const page = parseInt(pageInputValue.value)
  if (!isNaN(page) && page >= 1 && page <= (props.totalPages || 1)) {
    changePage(page)
  } else {
    // Reset to current page if invalid
    pageInputValue.value = (props.currentPage || 1).toString()
  }
}

const handlePageInputKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Enter') {
    handlePageInput()
    // Blur the input to remove focus after Enter
    if (pageInput.value) {
      pageInput.value.blur()
    }
  } else if (event.key === 'Escape') {
    pageInputValue.value = (props.currentPage || 1).toString()
    if (pageInput.value) {
      pageInput.value.blur()
    }
  }
}

// Auto-select content when focused for easy replacement
const handlePageInputFocus = (event: FocusEvent) => {
  const input = event.target as HTMLInputElement
  input.select()
}

// Generate smart page numbers for pagination
const pageNumbers = computed(() => {
  if (!props.totalPages || props.totalPages <= 1) return []
  
  const currentPage = props.currentPage || 1
  const totalPages = props.totalPages
  
  // For mobile, show fewer pages
  const maxVisible = isMobile.value ? 3 : 5
  
  if (totalPages <= maxVisible + 2) {
    // Show all pages if total is small
    return Array.from({ length: totalPages }, (_, i) => i + 1)
  }
  
  const pages: (number | string)[] = []
  
  // Always show first page
  pages.push(1)
  
  // Calculate range around current page
  const start = Math.max(2, currentPage - Math.floor(maxVisible / 2))
  const end = Math.min(totalPages - 1, currentPage + Math.floor(maxVisible / 2))
  
  // Add ellipsis after first page if needed
  if (start > 2) {
    pages.push('...')
  }
  
  // Add pages around current
  for (let i = start; i <= end; i++) {
    pages.push(i)
  }
  
  // Add ellipsis before last page if needed
  if (end < totalPages - 1) {
    pages.push('...')
  }
  
  // Always show last page if it's not already included
  if (totalPages > 1) {
    pages.push(totalPages)
  }
  
  return pages
})

// Default page size options if not provided
const defaultPageSizeOptions = [10, 25, 50, 100]

// Computed values for better UX
const currentPageDisplay = computed(() => props.currentPage || 1)
const totalPagesDisplay = computed(() => props.totalPages || 1)
const hasMultiplePages = computed(() => totalPagesDisplay.value > 1)
</script>

<template>
  <div class="flex-shrink-0 bg-surface border-t border-default">
    <!-- Mobile Layout -->
    <div v-if="isMobile" class="flex flex-col gap-3 p-3">
      <!-- Top row: Page info and navigation -->
      <div class="flex items-center justify-between">
        <!-- Page info with input -->
        <div class="flex items-center gap-2">
          <span class="text-sm text-secondary">Page</span>
          <div class="flex items-center gap-1">
            <input
              v-model="pageInputValue"
              @blur="handlePageInput"
              @keydown="handlePageInputKeydown"
              @focus="handlePageInputFocus"
              type="number"
              :min="1"
              :max="totalPagesDisplay"
              class="w-16 px-2 py-1 text-sm bg-surface-alt border border-default text-primary rounded focus:ring-blue-500 focus:border-blue-500 [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none font-mono text-center"
              ref="pageInput"
            />
            <span class="text-sm text-secondary">of {{ totalPagesDisplay }}</span>
          </div>
        </div>
        
        <!-- Navigation buttons -->
        <div v-if="hasMultiplePages" class="flex items-center gap-1">
          <button
            @click="changePage(currentPageDisplay - 1)"
            :disabled="currentPageDisplay <= 1"
            :class="[
              'p-2 rounded-md text-sm transition-colors',
              currentPageDisplay <= 1
                ? 'bg-surface-alt text-tertiary cursor-not-allowed'
                : 'bg-surface-alt text-primary hover:bg-surface-hover'
            ]"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" />
            </svg>
          </button>

          <button
            @click="changePage(currentPageDisplay + 1)"
            :disabled="currentPageDisplay >= totalPagesDisplay"
            :class="[
              'p-2 rounded-md text-sm transition-colors',
              currentPageDisplay >= totalPagesDisplay
                ? 'bg-surface-alt text-tertiary cursor-not-allowed'
                : 'bg-surface-alt text-primary hover:bg-surface-hover'
            ]"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
            </svg>
          </button>
        </div>
      </div>
      
      <!-- Bottom row: Page size and import -->
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2 text-sm text-secondary">
          <select
            :value="pageSize"
            @change="changePageSize"
            class="bg-surface-alt border border-default text-primary text-sm rounded-md focus:ring-blue-500 focus:border-blue-500 py-1 px-2"
          >
            <option
              v-for="size in (pageSizeOptions || defaultPageSizeOptions)"
              :key="size"
              :value="size"
            >
              {{ size }}
            </option>
          </select>
          <span>per page</span>
        </div>

        <button
          v-if="showImport"
          @click="emit('import')"
          class="px-3 py-1 text-xs font-medium text-primary bg-blue-600 rounded-md hover:bg-blue-700 focus:ring-2 focus:outline-none focus:ring-blue-800 flex items-center gap-1 transition-colors"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-8l-4-4m0 0l-4 4m4-4v12" />
          </svg>
          Import
        </button>
      </div>
    </div>

    <!-- Desktop Layout -->
    <div v-else class="flex items-center justify-between p-3">
      <!-- Left: Page size selector -->
      <div class="flex items-center gap-2 text-sm text-secondary min-w-0 flex-shrink-0">
        <span>Show</span>
        <select
          :value="pageSize"
          @change="changePageSize"
          class="bg-surface-alt border border-default text-primary text-sm rounded-md focus:ring-blue-500 focus:border-blue-500 py-1 px-2"
        >
          <option
            v-for="size in (pageSizeOptions || defaultPageSizeOptions)"
            :key="size"
            :value="size"
          >
            {{ size }}
          </option>
        </select>
        <span>per page</span>
      </div>

      <!-- Center: Page navigation (fixed width container) -->
      <div v-if="hasMultiplePages" class="flex items-center gap-4 min-w-0 flex-1 justify-center">
        <!-- Previous page button -->
        <button
          @click="changePage(currentPageDisplay - 1)"
          :disabled="currentPageDisplay <= 1"
          :class="[
            'p-2 rounded-md text-sm transition-colors flex-shrink-0',
            currentPageDisplay <= 1
              ? 'bg-surface-alt text-tertiary cursor-not-allowed'
              : 'bg-surface-alt text-primary hover:bg-surface-hover'
          ]"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" />
          </svg>
        </button>

        <!-- Page numbers container with FIXED width to prevent layout shifts -->
        <div class="flex items-center gap-1 w-96 justify-center">
          <template v-for="page in pageNumbers" :key="page">
            <button
              v-if="typeof page === 'number'"
              @click="changePage(page)"
              :class="[
                'py-1 text-sm rounded-md transition-colors flex-shrink-0 w-10 text-center',
                page === currentPageDisplay
                  ? 'bg-blue-600 text-primary'
                  : 'bg-surface-alt text-primary hover:bg-surface-hover'
              ]"
            >
              {{ page }}
            </button>
            <span
              v-else
              class="py-1 text-sm text-secondary flex-shrink-0 w-10 text-center"
            >
              ...
            </span>
          </template>
        </div>

        <!-- Next page button -->
        <button
          @click="changePage(currentPageDisplay + 1)"
          :disabled="currentPageDisplay >= totalPagesDisplay"
          :class="[
            'p-2 rounded-md text-sm transition-colors flex-shrink-0',
            currentPageDisplay >= totalPagesDisplay
              ? 'bg-surface-alt text-tertiary cursor-not-allowed'
              : 'bg-surface-alt text-primary hover:bg-surface-hover'
          ]"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
        </button>
      </div>

      <!-- Right: Page info and Import button -->
      <div class="flex items-center gap-3 min-w-0 flex-shrink-0">
        <!-- Page info with direct input -->
        <div class="flex items-center gap-2 text-sm text-secondary">
          <span>Page</span>
          <div class="flex items-center gap-1">
            <input
              v-model="pageInputValue"
              @blur="handlePageInput"
              @keydown="handlePageInputKeydown"
              @focus="handlePageInputFocus"
              type="number"
              :min="1"
              :max="totalPagesDisplay"
              class="px-2 py-1 text-sm bg-surface-alt border border-default text-primary rounded focus:ring-blue-500 focus:border-blue-500 [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none font-mono text-center"
              ref="pageInput"
            />
            <span>of {{ totalPagesDisplay }}</span>
          </div>
        </div>

        <!-- Import button -->
        <button
          v-if="showImport"
          @click="emit('import')"
          class="px-3 py-1 text-xs font-medium text-primary bg-blue-600 rounded-md hover:bg-blue-700 focus:ring-2 focus:outline-none focus:ring-blue-800 flex items-center gap-1 transition-colors"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-8l-4-4m0 0l-4 4m4-4v12" />
          </svg>
          Import
        </button>
      </div>
    </div>
  </div>
</template> 