<script setup lang="ts">
import { ref, onMounted, onActivated } from 'vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { useDocumentation } from '@/composables/useDocumentation'
import DocumentationCardGrid from '@/components/documentationComponents/DocumentationCardGrid.vue'
import DocumentationCardSkeleton from '@/components/documentationComponents/DocumentationCardSkeleton.vue'
import CollectionBrowser from '@/components/documentationComponents/CollectionBrowser.vue'
import { getArchivedPages, getTrashedPages } from '@/services/documentationService'
import { getUncollectedPages } from '@/services/collectionService'

const titleManager = useTitleManager()

// Use shared documentation composable
const {
  pages,
  showSkeleton,
  loadAllPages,
  createNewPage,
} = useDocumentation()

// Drafts count
const draftCount = ref(0)
const archivedCount = ref(0)
const trashCount = ref(0)

const loadDraftCount = async () => {
  const drafts = await getUncollectedPages()
  draftCount.value = drafts.length
}

const loadArchivedCount = async () => {
  const archived = await getArchivedPages()
  archivedCount.value = archived.length
}

const loadTrashCount = async () => {
  const trashed = await getTrashedPages()
  trashCount.value = trashed.length
}

// Handle page creation
const handleCreatePage = async () => {
  try {
    await createNewPage()
  } catch (error) {
    console.error('Failed to create page:', error)
  }
}

// Lifecycle
onMounted(async () => {
  titleManager.setCustomTitle('Documentation')
  await Promise.all([loadAllPages(), loadDraftCount(), loadArchivedCount(), loadTrashCount()])
})

onActivated(() => {
  // Refresh data when returning to this view (KeepAlive)
  loadAllPages()
  loadDraftCount()
  loadArchivedCount()
  loadTrashCount()
})

// Expose for parent components (SiteHeader create button)
defineExpose({
  createNewPage: handleCreatePage
})
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <!-- Main content -->
    <div class="flex flex-col flex-1 overflow-auto bg-gradient-to-b from-bg-app to-bg-surface items-center">
      <div class="flex flex-col max-w-7xl mx-auto w-full px-4 py-6 gap-6">
        <!-- Header -->
        <div class="flex items-center justify-between gap-4 pb-4 border-b border-default">
          <div class="flex items-center gap-2">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6 text-accent" viewBox="0 0 20 20" fill="currentColor">
              <path d="M2 6a2 2 0 012-2h5l2 2h5a2 2 0 012 2v6a2 2 0 01-2 2H4a2 2 0 01-2-2V6z" />
            </svg>
            <h2 class="text-xl font-semibold text-primary">Documentation</h2>
          </div>

          <!-- Page count badge -->
          <span
            class="text-xs bg-surface-alt px-2 py-1 rounded-full"
            :class="showSkeleton ? 'text-transparent animate-pulse' : 'text-tertiary'"
          >
            {{ showSkeleton ? '0 pages' : `${pages.length} page${pages.length !== 1 ? 's' : ''}` }}
          </span>
        </div>

        <!-- Collections -->
        <CollectionBrowser />

        <!-- Drafts Banner -->
        <RouterLink
          v-if="draftCount > 0"
          to="/documentation/drafts"
          class="flex items-center gap-3 px-4 py-2.5 rounded-lg bg-surface-alt hover:bg-surface-hover border border-default transition-colors group"
        >
          <span class="text-base">✏️</span>
          <span class="text-sm text-secondary group-hover:text-primary">
            You have <span class="font-medium text-primary">{{ draftCount }}</span> unpublished draft{{ draftCount !== 1 ? 's' : '' }}
          </span>
          <span class="ml-auto text-xs text-tertiary group-hover:text-accent transition-colors">View &rarr;</span>
        </RouterLink>

        <!-- Archived Banner -->
        <RouterLink
          v-if="archivedCount > 0"
          to="/documentation/archived"
          class="flex items-center gap-3 px-4 py-2.5 rounded-lg bg-surface-alt hover:bg-surface-hover border border-default transition-colors group"
        >
          <svg class="w-4 h-4 text-tertiary flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8m-9 4h4" />
          </svg>
          <span class="text-sm text-secondary group-hover:text-primary">
            <span class="font-medium text-primary">{{ archivedCount }}</span> archived page{{ archivedCount !== 1 ? 's' : '' }}
          </span>
          <span class="ml-auto text-xs text-tertiary group-hover:text-accent transition-colors">View &rarr;</span>
        </RouterLink>

        <!-- Trash Banner -->
        <RouterLink
          v-if="trashCount > 0"
          to="/documentation/trash"
          class="flex items-center gap-3 px-4 py-2.5 rounded-lg bg-surface-alt hover:bg-surface-hover border border-default transition-colors group"
        >
          <svg class="w-4 h-4 text-status-error/60 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
          </svg>
          <span class="text-sm text-secondary group-hover:text-primary">
            <span class="font-medium text-primary">{{ trashCount }}</span> page{{ trashCount !== 1 ? 's' : '' }} in trash
          </span>
          <span class="ml-auto text-xs text-tertiary group-hover:text-accent transition-colors">View &rarr;</span>
        </RouterLink>

        <!-- All Pages -->
        <div class="flex items-center justify-between gap-4 pb-4 border-b border-default">
          <div class="flex items-center gap-2">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-accent" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M4 4a2 2 0 012-2h4.586A2 2 0 0112 2.586L15.414 6A2 2 0 0116 7.414V16a2 2 0 01-2 2H6a2 2 0 01-2-2V4z" clip-rule="evenodd" />
            </svg>
            <h3 class="text-lg font-semibold text-primary">All Pages</h3>
          </div>
          <span
            class="text-xs bg-surface-alt px-2 py-1 rounded-full"
            :class="showSkeleton ? 'text-transparent animate-pulse' : 'text-tertiary'"
          >
            {{ showSkeleton ? '0 pages' : `${pages.length} page${pages.length !== 1 ? 's' : ''}` }}
          </span>
        </div>

        <!-- Card Grid or Skeleton -->
        <DocumentationCardSkeleton v-if="showSkeleton" :count="6" />
        <DocumentationCardGrid v-else :pages="pages" @create="handleCreatePage" />
      </div>
    </div>
  </div>
</template>
