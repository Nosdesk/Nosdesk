<script setup lang="ts">
import { computed, ref, onMounted } from 'vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { getArchivedPages, restorePage } from '@/services/documentationService'
import type { Page } from '@/services/documentationService'
import { useSSEListeners } from '@/composables/useSSEListeners'
import BackButton from '@/components/common/BackButton.vue'
import DocumentationRowSkeleton from '@/components/documentationComponents/DocumentationRowSkeleton.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import Icon from '@/components/common/Icon.vue'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import { formatDate } from '@/utils/dateUtils'
import { docUrl } from '@/utils/docUrl'

const titleManager = useTitleManager()
const docNavStore = useDocumentationNavStore()

// `initialLoading` gates the skeleton — only true on the first paint
// when we have no data at all. Background refetches triggered by SSE
// events leave the existing rows on screen so there's no flicker.
const initialLoading = ref(true)
const pages = ref<Page[]>([])

const showSkeleton = computed(() => initialLoading.value && pages.value.length === 0)

const loadArchivedPages = async () => {
  try {
    pages.value = await getArchivedPages()
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

// SSE integration for real-time updates
const { on, debouncedReload } = useSSEListeners({ reload: loadArchivedPages })

on('documentation-updated', (data) => {
  const event = data as { document_id: number; field: string; value: unknown }
  if (event.field !== 'status') return
  const statusVal = typeof event.value === 'string' ? event.value : String(event.value)
  if (statusVal === 'archived') {
    debouncedReload()
  } else {
    pages.value = pages.value.filter(p => p.id !== event.document_id)
  }
})

onMounted(() => {
  titleManager.setCustomTitle('Archived')
  loadArchivedPages()
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
            <span class="text-tertiary inline-flex">
              <Icon name="archive" size="lg" />
            </span>
            <div>
              <h2 class="text-xl font-semibold text-primary">Archived</h2>
              <p class="text-sm text-tertiary mt-0.5">Pages that have been archived</p>
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
        <DocumentationRowSkeleton v-if="showSkeleton" :count="4" label="Loading archived pages" />

        <!-- Empty state -->
        <EmptyState
          v-else-if="pages.length === 0"
          icon="inbox"
          title="No archived pages"
          description="Archived pages will appear here."
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
              <RouterLink
                :to="docUrl(page)"
                class="text-sm font-medium text-primary hover:text-accent truncate block"
              >
                {{ page.title }}
              </RouterLink>
              <span v-if="page.archived_at" class="text-xs text-tertiary">
                Archived {{ formatDate(page.archived_at) }}
              </span>
            </div>
            <button
              @click="handleRestore(page.id)"
              class="px-3 py-1.5 text-xs rounded-md border border-default text-secondary hover:text-primary hover:bg-surface-hover transition-colors flex items-center gap-1.5 flex-shrink-0"
            >
              <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 10h10a8 8 0 018 8v2M3 10l6 6m-6-6l6-6" />
              </svg>
              Restore
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
