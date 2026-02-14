<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { getArchivedPages, restorePage } from '@/services/documentationService'
import type { Page } from '@/services/documentationService'
import BackButton from '@/components/common/BackButton.vue'
import DocumentationCardSkeleton from '@/components/documentationComponents/DocumentationCardSkeleton.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import { formatDate } from '@/utils/dateUtils'

const titleManager = useTitleManager()
const docNavStore = useDocumentationNavStore()

const loading = ref(true)
const pages = ref<Page[]>([])

const loadArchivedPages = async () => {
  loading.value = true
  pages.value = await getArchivedPages()
  loading.value = false
}

const handleRestore = async (pageId: string | number) => {
  const success = await restorePage(pageId)
  if (success) {
    pages.value = pages.value.filter(p => String(p.id) !== String(pageId))
    docNavStore.refreshPages()
  }
}

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
            <svg class="h-7 w-7 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8m-9 4h4" />
            </svg>
            <div>
              <h2 class="text-xl font-semibold text-primary">Archived</h2>
              <p class="text-sm text-tertiary mt-0.5">Pages that have been archived</p>
            </div>
          </div>
          <span
            class="text-xs bg-surface-alt px-2 py-1 rounded-full"
            :class="loading ? 'text-transparent animate-pulse' : 'text-tertiary'"
          >
            {{ loading ? '0 pages' : `${pages.length} page${pages.length !== 1 ? 's' : ''}` }}
          </span>
        </div>

        <!-- Loading -->
        <DocumentationCardSkeleton v-if="loading" :count="4" />

        <!-- Empty state -->
        <EmptyState
          v-else-if="pages.length === 0"
          icon="inbox"
          title="No archived pages"
          description="Archived pages will appear here."
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
              <RouterLink
                :to="`/documentation/${page.slug || page.id}`"
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
