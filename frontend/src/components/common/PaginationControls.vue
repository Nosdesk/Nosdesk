<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import BaseDropdown from './BaseDropdown.vue'
import { useMobileDetection } from '@/composables/useMobileDetection'

const props = withDefaults(defineProps<{
  currentPage: number
  totalPages: number
  totalItems: number
  pageSize: number
  pageSizeOptions: number[]
  /** Whether infinite scroll mode is active (pageSize === 0) */
  isInfiniteMode: boolean
}>(), {
  currentPage: 1,
  totalPages: 1,
  totalItems: 0,
  pageSize: 25,
  isInfiniteMode: false
})

const emit = defineEmits<{
  'update:currentPage': [page: number]
  'update:pageSize': [size: number]
  'go-to-item': [itemId: number]
}>()

// Use shared mobile detection (md breakpoint = 768px for pagination)
const { isMobile } = useMobileDetection('md')

// Page input state
const pageInputValue = ref(props.currentPage.toString())
const pageInput = ref<HTMLInputElement | null>(null)

// Go-to-item input state
const goToItemValue = ref('')
const goToItemInput = ref<HTMLInputElement | null>(null)

// Watch for currentPage changes to update input value
watch(() => props.currentPage, (newPage) => {
  pageInputValue.value = newPage.toString()
}, { immediate: true })

// Pagination methods
const changePage = (page: number) => {
  if (page >= 1 && page <= props.totalPages) {
    emit('update:currentPage', page)
  }
}

const handlePageSizeChange = (value: string) => {
  emit('update:pageSize', parseInt(value))
}

// Handle direct page input
const handlePageInput = () => {
  const page = parseInt(pageInputValue.value)
  if (!isNaN(page) && page >= 1 && page <= props.totalPages) {
    changePage(page)
  } else {
    pageInputValue.value = props.currentPage.toString()
  }
}

const handlePageInputKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Enter') {
    handlePageInput()
    pageInput.value?.blur()
  } else if (event.key === 'Escape') {
    pageInputValue.value = props.currentPage.toString()
    pageInput.value?.blur()
  }
}

// Handle go-to-item input
const handleGoToItem = () => {
  const itemId = parseInt(goToItemValue.value)
  if (!isNaN(itemId) && itemId > 0) {
    emit('go-to-item', itemId)
    goToItemValue.value = ''
  }
}

const handleGoToItemKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Enter') {
    handleGoToItem()
    goToItemInput.value?.blur()
  } else if (event.key === 'Escape') {
    goToItemValue.value = ''
    goToItemInput.value?.blur()
  }
}

const handleInputFocus = (event: FocusEvent) => {
  (event.target as HTMLInputElement).select()
}

// Page numbers for pagination mode
const pageNumbers = computed(() => {
  if (props.totalPages <= 1) return []

  const maxVisible = isMobile.value ? 3 : 5

  if (props.totalPages <= maxVisible + 2) {
    return Array.from({ length: props.totalPages }, (_, i) => i + 1)
  }

  const pages: (number | string)[] = [1]
  const start = Math.max(2, props.currentPage - Math.floor(maxVisible / 2))
  const end = Math.min(props.totalPages - 1, props.currentPage + Math.floor(maxVisible / 2))

  if (start > 2) pages.push('...')
  for (let i = start; i <= end; i++) pages.push(i)
  if (end < props.totalPages - 1) pages.push('...')
  if (props.totalPages > 1) pages.push(props.totalPages)

  return pages
})

// Page size dropdown options
const pageSizeDropdownOptions = computed(() => {
  return props.pageSizeOptions.map(size => ({
    value: size.toString(),
    label: size === 0 ? 'All' : size.toString()
  }))
})

// Display helpers
const hasMultiplePages = computed(() => !props.isInfiniteMode && props.totalPages > 1)
</script>

<template>
  <div class="flex-shrink-0 bg-surface border-t border-default">
    <!-- Mobile Layout -->
    <div v-if="isMobile" class="flex items-center justify-between gap-2 p-2">
      <!-- Left: Position info -->
      <div class="flex items-center gap-1 text-xs text-secondary">
        <template v-if="isInfiniteMode">
          <span>{{ totalItems }} items</span>
        </template>
        <template v-else>
          <span>Page</span>
          <input
            v-model="pageInputValue"
            @blur="handlePageInput"
            @keydown="handlePageInputKeydown"
            @focus="handleInputFocus"
            type="number"
            :min="1"
            :max="totalPages"
            class="w-10 px-1 py-0.5 text-xs bg-surface-alt border border-default text-primary rounded focus:ring-accent focus:border-accent [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none font-mono text-center"
            ref="pageInput"
          />
          <span>/{{ totalPages }}</span>
        </template>
      </div>

      <!-- Center: Per page selector -->
      <BaseDropdown
        :model-value="pageSize.toString()"
        :options="pageSizeDropdownOptions"
        size="sm"
        @update:model-value="handlePageSizeChange"
      />

      <!-- Right: Navigation buttons (pagination mode only) -->
      <div v-if="hasMultiplePages" class="flex items-center gap-1">
        <button
          @click="changePage(currentPage - 1)"
          :disabled="currentPage <= 1"
          :class="[
            'p-1.5 rounded text-xs transition-colors',
            currentPage <= 1
              ? 'bg-surface-alt text-tertiary cursor-not-allowed'
              : 'bg-surface-alt text-primary hover:bg-surface-hover'
          ]"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" />
          </svg>
        </button>
        <button
          @click="changePage(currentPage + 1)"
          :disabled="currentPage >= totalPages"
          :class="[
            'p-1.5 rounded text-xs transition-colors',
            currentPage >= totalPages
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

    <!-- Desktop Layout -->
    <div v-else class="flex items-center justify-between p-3 gap-4">
      <!-- Left: Page size selector -->
      <div class="flex items-center gap-2 text-sm text-secondary flex-shrink-0">
        <span>Show</span>
        <BaseDropdown
          :model-value="pageSize.toString()"
          :options="pageSizeDropdownOptions"
          size="sm"
          @update:model-value="handlePageSizeChange"
        />
        <span>per page</span>
      </div>

      <!-- Center: Pagination controls -->
      <div class="flex-1 flex items-center justify-center min-w-0">
        <!-- Infinite scroll mode: Just show total -->
        <template v-if="isInfiniteMode">
          <span class="text-sm text-secondary">{{ totalItems }} items</span>
        </template>

        <!-- Pagination mode: Page numbers -->
        <template v-else-if="hasMultiplePages">
          <div class="flex items-center gap-2">
            <button
              @click="changePage(currentPage - 1)"
              :disabled="currentPage <= 1"
              :class="[
                'p-2 rounded-md text-sm transition-colors flex-shrink-0',
                currentPage <= 1
                  ? 'bg-surface-alt text-tertiary cursor-not-allowed'
                  : 'bg-surface-alt text-primary hover:bg-surface-hover'
              ]"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" />
              </svg>
            </button>

            <div class="flex items-center gap-1">
              <template v-for="page in pageNumbers" :key="page">
                <button
                  v-if="typeof page === 'number'"
                  @click="changePage(page)"
                  :class="[
                    'py-1 text-sm rounded-md transition-colors w-10 text-center',
                    page === currentPage
                      ? 'bg-accent text-white'
                      : 'bg-surface-alt text-primary hover:bg-surface-hover'
                  ]"
                >
                  {{ page }}
                </button>
                <span v-else class="py-1 text-sm text-secondary w-10 text-center">...</span>
              </template>
            </div>

            <button
              @click="changePage(currentPage + 1)"
              :disabled="currentPage >= totalPages"
              :class="[
                'p-2 rounded-md text-sm transition-colors flex-shrink-0',
                currentPage >= totalPages
                  ? 'bg-surface-alt text-tertiary cursor-not-allowed'
                  : 'bg-surface-alt text-primary hover:bg-surface-hover'
              ]"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
              </svg>
            </button>
          </div>
        </template>
      </div>

      <!-- Right: Go to item (infinite) or page info (pagination) -->
      <div class="flex items-center gap-3 flex-shrink-0">
        <template v-if="isInfiniteMode">
          <!-- Go to ticket input -->
          <div class="flex items-center gap-2 text-sm text-secondary">
            <span>Go to #</span>
            <input
              v-model="goToItemValue"
              @keydown="handleGoToItemKeydown"
              @focus="handleInputFocus"
              type="number"
              min="1"
              placeholder="ID"
              class="w-16 px-2 py-1 text-sm bg-surface-alt border border-default text-primary rounded focus:ring-accent focus:border-accent [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none font-mono text-center"
              ref="goToItemInput"
            />
          </div>
        </template>

        <template v-else>
          <!-- Page info with direct input -->
          <div class="flex items-center gap-2 text-sm text-secondary">
            <span>Page</span>
            <input
              v-model="pageInputValue"
              @blur="handlePageInput"
              @keydown="handlePageInputKeydown"
              @focus="handleInputFocus"
              type="number"
              :min="1"
              :max="totalPages"
              class="w-12 px-2 py-1 text-sm bg-surface-alt border border-default text-primary rounded focus:ring-accent focus:border-accent [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none font-mono text-center"
              ref="pageInput"
            />
            <span>of {{ totalPages }}</span>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>
