<script setup lang="ts">
import { computed, ref, onMounted } from 'vue'
import { useFluent } from 'fluent-vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { getTrashedPages, restorePage, permanentlyDeletePage } from '@/services/documentationService'
import type { Page } from '@/services/documentationService'
import { useSSEListeners } from '@/composables/useSSEListeners'
import BackButton from '@/components/common/BackButton.vue'
import DocumentationRowSkeleton from '@/components/documentationComponents/DocumentationRowSkeleton.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import Icon from '@/components/common/Icon.vue'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import { formatDate } from '@/utils/dateUtils'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

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
  titleManager.setCustomTitle(t('docs-trash-title'))
  loadTrashedPages()
})
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <!-- Header -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2">
        <BackButton fallbackRoute="/documentation" :label="$t('docs-trash-back')" />
        <div class="flex-1"></div>
      </div>
    </div>

    <!-- Main Content -->
    <div class="flex flex-col flex-1 overflow-auto bg-gradient-to-b from-bg-app to-bg-surface items-center">
      <div class="flex flex-col max-w-7xl mx-auto w-full px-4 py-6 gap-6">
        <!-- Header -->
        <div class="flex items-center justify-between gap-4 pb-4 border-b border-default">
          <div class="flex items-center gap-3">
            <span class="text-status-error inline-flex">
              <Icon name="trash" size="lg" />
            </span>
            <div>
              <h2 class="text-xl font-semibold text-primary">{{ $t('docs-trash-heading') }}</h2>
              <p class="text-sm text-tertiary mt-0.5">{{ $t('docs-trash-description') }}</p>
            </div>
          </div>
          <span
            v-if="!showSkeleton"
            class="text-xs bg-surface-alt px-2 py-1 rounded-full text-tertiary"
          >
            {{ $t('docs-trash-count', { count: pages.length }) }}
          </span>
        </div>

        <!-- Loading -->
        <DocumentationRowSkeleton
          v-if="showSkeleton"
          :count="4"
          :actions-per-row="2"
          :label="$t('docs-trash-loading')"
        />

        <!-- Empty state -->
        <EmptyState
          v-else-if="pages.length === 0"
          icon="trash"
          :title="$t('empty-documentation-trash-title')"
          :description="$t('empty-documentation-trash-description')"
          variant="card"
        />

        <!-- Page list -->
        <div v-else class="flex flex-col gap-2">
          <div
            v-for="page in pages"
            :key="page.id"
            class="flex items-center gap-3 px-4 py-3 bg-surface border border-default rounded-lg hover:bg-surface-hover transition-colors"
          >
            <span class="text-xl flex-shrink-0">{{ page.icon || '📄' }}</span>
            <div class="flex-1 min-w-0">
              <span class="text-sm font-medium text-primary truncate block">{{ page.title }}</span>
              <span v-if="page.deleted_at" class="text-xs text-tertiary">
                {{ $t('docs-trash-deleted-at', { date: formatDate(page.deleted_at) }) }}
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
                {{ $t('docs-trash-restore') }}
              </button>
              <button
                @click="handlePermanentDelete(page.id)"
                class="px-3 py-1.5 text-xs rounded-md transition-colors flex items-center gap-1.5"
                :class="String(confirmingDeleteId) === String(page.id)
                  ? 'bg-status-error text-white hover:bg-status-error/90'
                  : 'border border-default text-status-error hover:bg-status-error/10'"
              >
                <Icon name="trash" />
                {{ String(confirmingDeleteId) === String(page.id) ? $t('docs-trash-confirm-delete') : $t('docs-trash-delete-forever') }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
