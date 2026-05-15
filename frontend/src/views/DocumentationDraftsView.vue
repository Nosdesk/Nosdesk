<script setup lang="ts">
import { computed, ref, onMounted } from 'vue'
import { useFluent } from 'fluent-vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { getUncollectedPages } from '@/services/collectionService'
import type { CollectionPage } from '@/services/collectionService'
import { useSSEListeners } from '@/composables/useSSEListeners'
import BackButton from '@/components/common/BackButton.vue'
import DocumentationCardGrid from '@/components/documentationComponents/DocumentationCardGrid.vue'
import DocumentationCardSkeleton from '@/components/documentationComponents/DocumentationCardSkeleton.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const titleManager = useTitleManager()

// Skeleton only on the first paint; SSE refetches keep the current
// cards on screen to avoid flicker. Matches the Archived / Trash
// views' stale-while-revalidate treatment.
const initialLoading = ref(true)
const pagesForGrid = ref<any[]>([])

const showSkeleton = computed(() => initialLoading.value && pagesForGrid.value.length === 0)

const loadDrafts = async () => {
  try {
    const drafts = await getUncollectedPages()
    pagesForGrid.value = drafts.map((p: CollectionPage) => ({
      ...p,
      children: [],
      author: '',
      content: '',
      description: null,
    }))
  } finally {
    initialLoading.value = false
  }
}

// SSE integration for real-time updates
const { on, debouncedReload } = useSSEListeners({ reload: loadDrafts })

on('documentation-updated', (data) => {
  const event = data as { document_id: number; field: string; value: unknown }
  if (event.field !== 'status') return
  const statusVal = typeof event.value === 'string' ? event.value : String(event.value)
  const idx = pagesForGrid.value.findIndex(p => p.id === event.document_id)
  if (statusVal === 'draft' && idx === -1) {
    debouncedReload()
  } else if (statusVal !== 'draft' && idx !== -1) {
    pagesForGrid.value.splice(idx, 1)
  }
})

on('documentation-created', () => {
  debouncedReload()
})

onMounted(() => {
  titleManager.setCustomTitle(t('docs-drafts-title'))
  loadDrafts()
})
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <!-- Header -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2">
        <BackButton fallbackRoute="/documentation" :label="$t('docs-drafts-back')" />
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
              <h2 class="text-xl font-semibold text-primary">{{ $t('docs-drafts-heading') }}</h2>
              <p class="text-sm text-tertiary mt-0.5">{{ $t('docs-drafts-description') }}</p>
            </div>
          </div>
          <span
            v-if="!showSkeleton"
            class="text-xs bg-surface-alt px-2 py-1 rounded-full text-tertiary"
          >
            {{ $t('docs-drafts-count', { count: pagesForGrid.length }) }}
          </span>
        </div>

        <!-- Pages -->
        <DocumentationCardSkeleton v-if="showSkeleton" :count="6" />
        <DocumentationCardGrid v-else :pages="pagesForGrid" />
      </div>
    </div>
  </div>
</template>
