<script setup lang="ts">
import { computed } from 'vue'
import EmptyState from './EmptyState.vue'
import ErrorBanner from './ErrorBanner.vue'

const props = withDefaults(defineProps<{
  title: string
  isLoading: boolean
  isEmpty: boolean
  emptyMessage?: string
  emptyDescription?: string
  emptyIcon?: 'folder' | 'document' | 'users' | 'device' | 'ticket' | 'search'
  emptyActionLabel?: string
  error?: string | null
  /** Whether to show mobile card view (controlled by parent) */
  isMobile?: boolean
  /** Whether infinite scroll is loading more items */
  isLoadingMore?: boolean
}>(), {
  isMobile: false,
  isLoadingMore: false
})

const emit = defineEmits<{
  'retry': []
  'empty-action': []
}>()

const defaultEmptyMessage = computed(() => {
  return `No ${props.title.toLowerCase()} found.`
})
</script>

<template>
  <div class="flex flex-col flex-1 min-h-0">
    <!-- Optional header slot for custom content -->
    <slot name="header-extra"></slot>

    <!-- Main content container -->
    <div class="flex-1 flex flex-col overflow-hidden">
      <!-- Error state -->
      <div v-if="error" class="p-4">
        <ErrorBanner
          :message="error"
          :show-retry="true"
          @retry="emit('retry')"
        />
      </div>

      <!-- Empty state - only show when not loading and truly empty -->
      <EmptyState
        v-else-if="isEmpty && !isLoading"
        :icon="emptyIcon"
        :title="emptyMessage || defaultEmptyMessage"
        :description="emptyDescription"
        :action-label="emptyActionLabel"
        @action="emit('empty-action')"
      />

      <!-- Content container - show during loading and when items exist -->
      <div v-else class="flex-1 flex flex-col overflow-hidden">
        <!-- Desktop Table View -->
        <div v-show="!isMobile" class="flex-1 flex flex-col overflow-hidden">
          <slot></slot>
        </div>

        <!-- Mobile Card View -->
        <div v-show="isMobile" class="flex-1 flex flex-col overflow-hidden">
          <slot name="mobile-view"></slot>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.flex-1 {
  flex: 1 1 0%;
}
</style>
