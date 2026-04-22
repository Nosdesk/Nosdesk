<script setup lang="ts">
import { computed, ref, onMounted } from 'vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { getTrashedPages, restorePage, permanentlyDeletePage } from '@/services/documentationService'
import type { Page } from '@/services/documentationService'
import { useSSEListeners } from '@/composables/useSSEListeners'
import BackButton from '@/components/common/BackButton.vue'
import DocumentationRowSkeleton from '@/components/documentationComponents/DocumentationRowSkeleton.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import { formatDate } from '@/utils/dateUtils'

const titleManager = useTitleManager()
const docNavStore = useDocumentationNavStore()

// See the matching comment in DocumentationArchivedView — skeleton is
// first-paint only, SSE-driven refetches do not blank the list.
const initialLoading = ref(true)
const pages = ref<Page[]>([])
const confirmingDeleteId = ref<string | number | null>(null)

const showSkeleton = computed(() => initialLoading.value && pages.value.length === 0)

const loadTrashedPages = async () => {
  try {
    pages.value = await getTrashedPages()
  } finally {
    initialLoading.value = false
  }
}

const handleRestore = async (pageId: string | number) => {
  const success = await restorePage(pageId)
  if (success) {
    pages.value = pages.value.filter(p => String(p.id) !== String(pageId))
    docNavStore.refreshPages()
  }
}

const handlePermanentDelete = async (pageId: string | number) => {
  if (String(confirmingDeleteId.value) !== String(pageId)) {
    confirmingDeleteId.value = pageId
    return
  }
  const success = await permanentlyDeletePage(pageId)
  if (success) {
    pages.value = pages.value.filter(p => String(p.id) !== String(pageId))
    confirmingDeleteId.value = null
  }
}

// SSE integration for real-time updates
const { on, debouncedReload } = useSSEListeners({ reload: loadTrashedPages })

on('documentation-updated', (data) => {
  const event = data as { document_id: number; field: string; value: unknown }
  if (event.field !== 'status') return
  const statusVal = typeof event.value === 'string' ? event.value : String(event.value)
  if (statusVal === 'deleted') {
    debouncedReload()
  } else {
    pages.value = pages.value.filter(p => p.id !== event.document_id)
  }
})

onMounted(() => {
  titleManager.setCustomTitle('Trash')
  loadTrashedPages()
})
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <!-- Header -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2">
        <BackButton fallbackRoute="/documentation" label="Back to Documentation" />
        <div class="flex-1"></div>
      </div>
    </div>

    <!-- Main Content -->
    <div class="flex flex-col flex-1 overflow-auto bg-gradient-to-b from-bg-app to-bg-surface items-center">
      <div class="flex flex-col max-w-7xl mx-auto w-full px-4 py-6 gap-6">
        <!-- Header -->
        <div class="flex items-center justify-between gap-4 pb-4 border-b border-default">
          <div class="flex items-center gap-3">
            <svg class="h-7 w-7 text-status-error" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
            </svg>
            <div>
              <h2 class="text-xl font-semibold text-primary">Trash</h2>
              <p class="text-sm text-tertiary mt-0.5">Deleted pages can be restored or permanently removed</p>
            </div>
          </div>
          <span
            v-if="!showSkeleton"
            class="text-xs bg-surface-alt px-2 py-1 rounded-full text-tertiary"
          >
            {{ pages.length }} page{{ pages.length !== 1 ? 's' : '' }}
          </span>
        </div>

        <!-- Loading -->
        <DocumentationRowSkeleton
          v-if="showSkeleton"
          :count="4"
          :actions-per-row="2"
          label="Loading trashed pages"
        />

        <!-- Empty state -->
        <EmptyState
          v-else-if="pages.length === 0"
          icon="trash"
          title="Trash is empty"
          description="Deleted pages will appear here."
          variant="card"
        />

        <!-- Page list -->
        <div v-else class="space-y-2">
          <div
            v-for="page in pages"
            :key="page.id"
            class="flex items-center gap-3 px-4 py-3 bg-surface border border-default rounded-lg hover:bg-surface-hover transition-colors"
          >
            <span class="text-xl flex-shrink-0">{{ page.icon || '📄' }}</span>
            <div class="flex-1 min-w-0">
              <span class="text-sm font-medium text-primary truncate block">{{ page.title }}</span>
              <span v-if="page.deleted_at" class="text-xs text-tertiary">
                Deleted {{ formatDate(page.deleted_at) }}
              </span>
            </div>
            <div class="flex items-center gap-2 flex-shrink-0">
              <button
                @click="handleRestore(page.id)"
                class="px-3 py-1.5 text-xs rounded-md border border-default text-secondary hover:text-primary hover:bg-surface-hover transition-colors flex items-center gap-1.5"
              >
                <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 10h10a8 8 0 018 8v2M3 10l6 6m-6-6l6-6" />
                </svg>
                Restore
              </button>
              <button
                @click="handlePermanentDelete(page.id)"
                class="px-3 py-1.5 text-xs rounded-md transition-colors flex items-center gap-1.5"
                :class="String(confirmingDeleteId) === String(page.id)
                  ? 'bg-status-error text-white hover:bg-status-error/90'
                  : 'border border-default text-status-error hover:bg-status-error/10'"
              >
                <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                </svg>
                {{ String(confirmingDeleteId) === String(page.id) ? 'Confirm delete?' : 'Delete forever' }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
