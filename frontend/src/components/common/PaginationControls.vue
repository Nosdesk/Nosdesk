<!--
List footer: page size, page navigation and position. Navigation is
Reka's Pagination: it computes the page list (edges, siblings, ellipses),
the `nav` landmark, `aria-current=page` on the current page and the
prev/next disabled states. Our `totalPages` is backend-authoritative and
can disagree with `totalItems / pageSize`, so the root is fed
`total=totalPages` with one item per page, which makes Reka's page count
ours exactly. Infinite mode (pageSize 0) renders no navigation.
-->
<script setup lang="ts">
import IconButton from '@/components/common/IconButton.vue'
import { computed, ref, watch } from 'vue'
import {
  PaginationEllipsis,
  PaginationList,
  PaginationListItem,
  PaginationNext,
  PaginationPrev,
  PaginationRoot,
} from 'reka-ui'
import { useFluent } from 'fluent-vue'
import BaseDropdown from './BaseDropdown.vue'
import { useMobileDetection } from '@/composables/useMobileDetection'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const props = withDefaults(defineProps<{
  currentPage: number
  totalPages: number
  totalItems: number
  pageSize: number
  pageSizeOptions: readonly number[]
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
}>()

// Use shared mobile detection (md breakpoint = 768px for pagination)
const { isMobile } = useMobileDetection('md')

// Page input state
const pageInputValue = ref(props.currentPage.toString())
const pageInput = ref<HTMLInputElement | null>(null)

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

const handlePageSizeChange = (value: string | string[]) => {
  const v = Array.isArray(value) ? value[0] : value
  emit('update:pageSize', parseInt(v))
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

const handleInputFocus = (event: FocusEvent) => {
  (event.target as HTMLInputElement).select()
}

// Reka lists every page when the count fits, else edges + siblings
// around the current page with ellipses; fewer siblings on phones.
const siblingCount = computed(() => (isMobile.value ? 1 : 2))

// Page size dropdown options
const pageSizeDropdownOptions = computed(() => {
  return props.pageSizeOptions.map(size => ({
    value: size.toString(),
    label: size === 0 ? t('pagination-controls-page-size-all') : size.toString()
  }))
})

// Display helpers
const hasMultiplePages = computed(() => !props.isInfiniteMode && props.totalPages > 1)
</script>

<template>
  <div class="flex-shrink-0 bg-surface border-t border-default">
    <!-- Mobile Layout -->
    <div v-if="isMobile" class="flex items-center justify-between gap-2 px-2 py-1.5">
      <!-- Left: Position info -->
      <div class="flex items-center gap-1 text-xs text-secondary">
        <template v-if="isInfiniteMode">
          <span>{{ t('pagination-controls-items', { count: totalItems }) }}</span>
        </template>
        <template v-else>
          <span>{{ t('pagination-controls-page') }}</span>
          <input
            v-model="pageInputValue"
            @blur="handlePageInput"
            @keydown="handlePageInputKeydown"
            @focus="handleInputFocus"
            type="number"
            :min="1"
            :max="totalPages"
            :aria-label="t('pagination-controls-page-input-aria')"
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
      <PaginationRoot
        v-if="hasMultiplePages"
        :page="currentPage"
        :total="totalPages"
        :items-per-page="1"
        :aria-label="t('pagination-controls-nav-aria')"
        class="flex items-center gap-1"
        @update:page="changePage"
      >
        <PaginationPrev as-child>
          <IconButton
            :label="$t('pagination-controls-previous')"
            icon="chevronLeft"
            variant="secondary"
            size="sm"
            :disabled="currentPage <= 1"
          />
        </PaginationPrev>
        <PaginationNext as-child>
          <IconButton
            :label="$t('pagination-controls-next')"
            icon="chevronRight"
            variant="secondary"
            size="sm"
            :disabled="currentPage >= totalPages"
          />
        </PaginationNext>
      </PaginationRoot>
    </div>

    <!-- Desktop Layout -->
    <div v-else class="flex items-center justify-between px-3 py-1.5 gap-4">
      <!-- Left: Page size selector -->
      <div class="flex items-center gap-1.5 text-sm text-secondary flex-shrink-0">
        <span>{{ t('pagination-controls-show') }}</span>
        <BaseDropdown
          :model-value="pageSize.toString()"
          :options="pageSizeDropdownOptions"
          size="xs"
          @update:model-value="handlePageSizeChange"
        />
        <span>{{ t('pagination-controls-per-page') }}</span>
      </div>

      <!-- Center: navigation only.
           The three slots each mean one thing: page size on the left,
           navigation in the middle, position on the right. The item count
           used to live here, which put it dead centre while the right slot
           collapsed to an empty div, because its only child is gated on
           `!isInfiniteMode`. Since infinite mode is the DEFAULT (pageSize
           starts at 0), that lopsided bar was what every list view showed
           out of the box. The count is position, not navigation, so it now
           sits on the right beside where "Page x of y" goes. -->
      <div class="flex-1 flex items-center justify-center min-w-0">
        <!-- Pagination mode: Page numbers -->
        <PaginationRoot
          v-if="hasMultiplePages"
          :page="currentPage"
          :total="totalPages"
          :items-per-page="1"
          :sibling-count="siblingCount"
          show-edges
          :aria-label="t('pagination-controls-nav-aria')"
          class="flex items-center gap-2"
          @update:page="changePage"
        >
          <PaginationPrev as-child>
            <IconButton
              :label="$t('pagination-controls-previous')"
              icon="chevronLeft"
              variant="secondary"
              size="sm"
              :disabled="currentPage <= 1"
            />
          </PaginationPrev>

          <PaginationList v-slot="{ items }" class="flex items-center gap-0.5">
            <template v-for="(item, index) in items" :key="index">
              <PaginationListItem v-if="item.type === 'page'" :value="item.value" as-child>
                <button
                  type="button"
                  :aria-label="t('pagination-controls-page-n', { page: item.value })"
                  class="py-0.5 text-sm rounded transition-colors w-8 text-center bg-surface-alt text-primary hover:bg-surface-hover data-[selected]:bg-accent data-[selected]:text-on-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                >
                  {{ item.value }}
                </button>
              </PaginationListItem>
              <PaginationEllipsis v-else class="text-sm text-secondary w-6 text-center" aria-hidden="true">
                …
              </PaginationEllipsis>
            </template>
          </PaginationList>

          <PaginationNext as-child>
            <IconButton
              :label="$t('pagination-controls-next')"
              icon="chevronRight"
              variant="secondary"
              size="sm"
              :disabled="currentPage >= totalPages"
            />
          </PaginationNext>
        </PaginationRoot>
      </div>

      <!-- Right: page info. The infinite-mode branch used to hold a
           "Go to #" input, removed because no consumer ever listened
           for the `go-to-item` it emitted, and its label was
           hard-coded English. -->
      <div class="flex items-center gap-2 flex-shrink-0">
        <!-- Infinite mode has no page to report, so the total takes the
             position slot: same place, same kind of information. -->
        <template v-if="isInfiniteMode">
          <span class="text-sm text-secondary">{{ t('pagination-controls-items', { count: totalItems }) }}</span>
        </template>

        <template v-else>
          <!-- Page info with direct input -->
          <div class="flex items-center gap-1.5 text-sm text-secondary">
            <span>{{ t('pagination-controls-page') }}</span>
            <input
              v-model="pageInputValue"
              @blur="handlePageInput"
              @keydown="handlePageInputKeydown"
              @focus="handleInputFocus"
              type="number"
              :min="1"
              :max="totalPages"
            :aria-label="t('pagination-controls-page-input-aria')"
              class="w-10 px-1.5 py-0.5 text-sm bg-surface-alt border border-default text-primary rounded focus:ring-accent focus:border-accent [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none font-mono text-center"
              ref="pageInput"
            />
            <span>{{ t('pagination-controls-of-total', { total: totalPages }) }}</span>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>
