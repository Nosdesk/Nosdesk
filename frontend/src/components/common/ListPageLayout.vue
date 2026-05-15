<!--
Page chrome for a list view (tickets / users / devices / future).

Pairs with `useListPage` (data orchestration) and the per-feature
`*Keys` factory in `@/queries`. The composable hands the layout a
`page` bundle (items, loading flags, error, retry); the layout
renders the sticky header, search input, filter row, bulk-action
bar, mobile/desktop toggle, error/empty/skeleton states, and the
pagination footer.

Critical: the layout owns the `is-empty` calculation
  is-empty = !!error || (items.length === 0 && !isFirstLoad)
so the loading-state contract documented in `useListPage` lives in
exactly one place. The "no flash on remount" property the recent
fix introduced depends on this exact rule, do not regress.

Slot map:
 - `#search-meta`         right-aligned meta (e.g. "X results")
 - `#filters`             filter pickers (BaseDropdown row)
 - `#bulk-actions`        BulkActionBar action buttons
 - `#desktop`             desktop body (the view's own DataTable
                          + cell templates lives here)
 - `#mobile-row="{ item, index }"`
                          one mobile row template; layout owns the
                          v-for + TransitionGroup + stagger
 - `#empty-state`         override the default EmptyState card
 - `#footer`              pagination + bulk modals
-->
<script setup lang="ts" generic="T">
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import PageScroll from '@/components/common/PageScroll.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import ErrorBanner from '@/components/common/ErrorBanner.vue'
import BulkActionBar from '@/components/common/BulkActionBar.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import { useStaggeredList } from '@/composables/useStaggeredList'
import { useMobileDetection } from '@/composables/useMobileDetection'
import type { BulkSelection } from '@/composables/useBulkSelection'

/**
 * Public shape exposed via `defineExpose`. Consumers type their
 * `useTemplateRef('layout')` against this rather than against
 * `InstanceType<typeof ListPageLayout>`, which TypeScript can't
 * resolve for generic components (they're not class-like).
 *
 * Note: `defineExpose` auto-unwraps refs at the consumer boundary,
 * so the exposed `scrollContainerRef` is typed as the unwrapped
 * `HTMLElement | null`, not the underlying `ComputedRef`.
 */
export interface ListPageLayoutExpose {
  scrollContainerRef: HTMLElement | null
}

const props = withDefaults(
  defineProps<{
    /** Reactive items list from `useListPage().items`. */
    items: readonly T[]
    /** Reactive count from `useListPage().totalItems`. */
    totalItems: number
    /** First-load skeleton signal from `useListPage().isFirstLoad`. */
    isFirstLoad: boolean
    /** Background-refresh signal from `useListPage().isBackgroundRefresh`. */
    isBackgroundRefresh: boolean
    /** Bottom-of-list spinner signal from `useListPage().isLoadingMore`. */
    isLoadingMore: boolean
    /** Error message from `useListPage().errorMessage` (null when ok). */
    error?: string | null
    /** Two-way bound search input value. */
    searchQuery?: string
    /** Search box placeholder. */
    searchPlaceholder?: string
    /** Per-domain item label (singular), e.g. `"ticket"`. Used for
     *  the bulk bar's "X tickets selected" copy. */
    itemLabel?: string
    /** Empty-state card props (used when no `#empty-state` slot). */
    emptyIcon?: 'key' | 'inbox' | 'search' | 'link' | 'folder' | 'document' | 'users' | 'device' | 'ticket' | 'calendar' | 'trash' | 'plugin'
    emptyTitle?: string
    emptyDescription?: string
    /** Bulk selection bundle from `useBulkSelection`. When provided
     *  the layout renders the BulkActionBar and exposes the
     *  `#bulk-actions` slot. Omit to disable bulk UI entirely. */
    bulkSelection?: BulkSelection<T>
  }>(),
  {
    error: null,
    searchQuery: '',
    searchPlaceholder: undefined,
    itemLabel: 'item',
    emptyIcon: 'inbox',
    emptyTitle: 'Nothing here yet',
    emptyDescription: '',
  },
)

const emit = defineEmits<{
  'update:search-query': [value: string]
  'retry': []
  'select-all-matching': []
  'clear-selection': []
}>()

// Declare slot signatures so consumer templates infer `item: T`
// (and other slot props) without `(item as Device)` casts. Vue
// can't deduce slot prop types from `<slot :item="item" />` tags
// alone, the contract has to be declared explicitly.
defineSlots<{
  filters(): unknown
  'search-meta'(): unknown
  'empty-state'(): unknown
  desktop(props: { items: readonly T[]; isBackgroundRefresh: boolean }): unknown
  'mobile-row'(props: { item: T; index: number }): unknown
  'bulk-actions'(props: { selectedCount: number; isAllMatching: boolean }): unknown
  footer(): unknown
}>()

const fluent = useFluent()
const resolvedSearchPlaceholder = computed(() => props.searchPlaceholder ?? fluent.$t('common-search-placeholder'))

const { isMobile } = useMobileDetection()
const { getStyle } = useStaggeredList()

const pageScrollRef = ref<InstanceType<typeof PageScroll> | null>(null)
/** Exposed so the consuming view can pass it to `useListPage`'s
 *  `scrollContainerRef` option. Avoids the consumer needing to
 *  `ref` the layout itself just to reach the scroll container. */
const scrollContainerRef = computed<HTMLElement | null>(
  () => pageScrollRef.value?.scrollContainerRef ?? null,
)

defineExpose({ scrollContainerRef })

/** Stable key extractor for the mobile v-for. Casts at one
 *  callsite instead of inline in the template (template TypeScript
 *  parsing can't resolve `as` expressions in directive arguments). */
function rowKey(item: T, index: number): string {
  const maybeId = (item as { id?: unknown }).id
  return maybeId === undefined ? String(index) : String(maybeId)
}

// `is-empty` controls whether PageScroll renders the empty slot
// (which carries the error/empty card) vs the body. The same rule
// the recent flash-on-remount fix introduced lives here, in one
// place: error or (no items AND past first-load).
const isEmpty = computed(
  () => !!props.error || (props.items.length === 0 && !props.isFirstLoad),
)

// Pluralise the item label for "X tickets" copy in the bulk bar.
function onSelectAllMatching() {
  emit('select-all-matching')
  props.bulkSelection?.selectAllMatching()
}
function onClearSelection() {
  emit('clear-selection')
  props.bulkSelection?.clear()
}
</script>

<template>
  <PageScroll
    ref="pageScrollRef"
    content-class="flex h-full flex-col"
    :is-empty="isEmpty"
  >
    <template #chrome>
      <!-- Sticky filter / search bar. The shadow + sticky combo
           anchors filters to the viewport top while the body
           scrolls behind. -->
      <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
        <div class="p-2 flex items-center gap-2 flex-wrap">
          <DebouncedSearchInput
            :model-value="searchQuery"
            @update:model-value="(value: string) => emit('update:search-query', value)"
            :placeholder="resolvedSearchPlaceholder"
            class="hidden sm:block"
          />

          <slot name="filters" />

          <div class="text-xs text-secondary ml-auto">
            <slot name="search-meta">
              {{ totalItems }} result{{ totalItems !== 1 ? 's' : '' }}
            </slot>
          </div>
        </div>
      </div>
    </template>

    <!-- Error / empty state. Errors take priority over empty so
         the user sees the underlying failure before the empty
         copy. The default EmptyState can be overridden via the
         `#empty-state` slot for per-view copy / call-to-action. -->
    <template #empty>
      <ErrorBanner
        v-if="error"
        :message="error"
        :show-retry="true"
        @retry="emit('retry')"
      />
      <slot v-else name="empty-state">
        <EmptyState
          :icon="emptyIcon"
          :title="emptyTitle"
          :description="emptyDescription"
        />
      </slot>
    </template>

    <!-- Desktop body. Hidden on mobile via v-show (rather than
         v-if) so the view's local component state survives the
         responsive breakpoint flip. -->
    <div v-show="!isMobile" class="flex h-full flex-col">
      <slot
        name="desktop"
        :items="items"
        :is-background-refresh="isBackgroundRefresh"
      />

      <!-- Infinite-scroll bottom spinner. Renders when a follow-up
           page is being fetched; the user keeps seeing existing
           rows above. -->
      <div v-if="isLoadingMore" class="py-4 flex justify-center bg-app">
        <div
          class="animate-spin rounded-full h-6 w-6 border-t-2 border-b-2 border-accent"
          aria-label="Loading more"
        />
      </div>
    </div>

    <!-- Mobile body. Layout owns the v-for + stagger + transition;
         the view's `#mobile-row` template renders one row given
         `{ item, index }`. -->
    <div v-show="isMobile" class="flex h-full flex-col">
      <TransitionGroup name="list-stagger" tag="div" class="flex flex-col">
        <div
          v-for="(item, index) in items"
          :key="rowKey(item, index)"
          :style="getStyle(index)"
        >
          <slot name="mobile-row" :item="item" :index="index" />
        </div>
      </TransitionGroup>

      <!-- Mobile also needs the bottom spinner during loadMore. -->
      <div v-if="isLoadingMore" class="py-4 flex justify-center bg-app">
        <div
          class="animate-spin rounded-full h-6 w-6 border-t-2 border-b-2 border-accent"
          aria-label="Loading more"
        />
      </div>
    </div>

    <!-- Bulk action bar. Sticky-bottom floating pill (Linear /
         Asana / Notion pattern); rendered at the body root level
         via `Teleport` from BulkActionBar so the page scroll
         doesn't clip it. -->
    <BulkActionBar
      v-if="bulkSelection"
      :selected-count="bulkSelection.selectedCount.value"
      :total-count="totalItems"
      :is-all-matching-selected="bulkSelection.isAllMatchingSelected.value"
      :item-label="itemLabel"
      @select-all-matching="onSelectAllMatching"
      @clear="onClearSelection"
    >
      <template #actions="{ selectedCount, isAllMatching }">
        <slot
          name="bulk-actions"
          :selected-count="selectedCount"
          :is-all-matching="isAllMatching"
        />
      </template>
    </BulkActionBar>

    <template #footer>
      <slot name="footer" />
    </template>
  </PageScroll>
</template>
