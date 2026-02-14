<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { getUncollectedPages } from '@/services/collectionService'
import type { CollectionPage } from '@/services/collectionService'
import BackButton from '@/components/common/BackButton.vue'
import DocumentationCardGrid from '@/components/documentationComponents/DocumentationCardGrid.vue'
import DocumentationCardSkeleton from '@/components/documentationComponents/DocumentationCardSkeleton.vue'

const titleManager = useTitleManager()

const loading = ref(true)
const pagesForGrid = ref<any[]>([])

const loadDrafts = async () => {
  loading.value = true
  const drafts = await getUncollectedPages()
  pagesForGrid.value = drafts.map((p: CollectionPage) => ({
    ...p,
    children: [],
    author: '',
    content: '',
    description: null,
  }))
  loading.value = false
}

onMounted(() => {
  titleManager.setCustomTitle('Drafts')
  loadDrafts()
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
        <!-- Drafts Header -->
        <div class="flex items-center justify-between gap-4 pb-4 border-b border-default">
          <div class="flex items-center gap-3">
            <span class="text-3xl">✏️</span>
            <div>
              <h2 class="text-xl font-semibold text-primary">Drafts</h2>
              <p class="text-sm text-tertiary mt-0.5">Pages not yet assigned to a collection</p>
            </div>
          </div>
          <span
            class="text-xs bg-surface-alt px-2 py-1 rounded-full"
            :class="loading ? 'text-transparent animate-pulse' : 'text-tertiary'"
          >
            {{ loading ? '0 pages' : `${pagesForGrid.length} page${pagesForGrid.length !== 1 ? 's' : ''}` }}
          </span>
        </div>

        <!-- Pages -->
        <DocumentationCardSkeleton v-if="loading" :count="6" />
        <DocumentationCardGrid v-else :pages="pagesForGrid" />
      </div>
    </div>
  </div>
</template>
