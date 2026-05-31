<script setup lang="ts">
import { computed, ref, onMounted } from 'vue'
import { RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { getCollections, createCollection, reorderCollections } from '@/services/collectionService'
import type { CollectionWithDetails } from '@/services/collectionService'
import { useAuthStore } from '@/stores/auth'
import DocumentIconSelector from '@/components/DocumentIconSelector.vue'
import Skeleton from '@/components/common/Skeleton.vue'
import SkeletonBar from '@/components/common/SkeletonBar.vue'
import Icon from '@/components/common/Icon.vue'

useFluent()
const authStore = useAuthStore()
const collections = ref<CollectionWithDetails[]>([])
const loading = ref(true)
const showCreateForm = ref(false)
const newCollectionName = ref('')
const newCollectionIcon = ref('📁')

// Drag state
const dragIndex = ref<number | null>(null)
const dropIndex = ref<number | null>(null)

const canReorder = authStore.isTechnician

// Only show the skeleton on the first load; background refetches
// (e.g. after a create / reorder) keep the existing cards on screen
// so nothing flashes.
const showSkeleton = computed(() => loading.value && collections.value.length === 0)
// Seed the skeleton count from the previous render when we have one,
// fall back to 3 for the cold first paint.
const skeletonCount = computed(() =>
  collections.value.length > 0 ? Math.min(collections.value.length, 6) : 3,
)

const loadCollections = async () => {
  loading.value = true
  try {
    collections.value = await getCollections()
  } finally {
    loading.value = false
  }
}

const handleCreate = async () => {
  if (!newCollectionName.value.trim()) return

  const result = await createCollection({
    name: newCollectionName.value.trim(),
    icon: newCollectionIcon.value || '📁',
  })

  if (result) {
    showCreateForm.value = false
    newCollectionName.value = ''
    newCollectionIcon.value = '📁'
    await loadCollections()
  }
}

// Drag and drop handlers
const onDragStart = (index: number, event: DragEvent) => {
  dragIndex.value = index
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', String(index))
  }
}

const onDragOver = (index: number, event: DragEvent) => {
  event.preventDefault()
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'move'
  }
  if (dragIndex.value !== null && dragIndex.value !== index) {
    dropIndex.value = index
  }
}

const onDragLeave = () => {
  dropIndex.value = null
}

const onDrop = async (targetIndex: number) => {
  if (dragIndex.value === null || dragIndex.value === targetIndex) {
    dragIndex.value = null
    dropIndex.value = null
    return
  }

  // Reorder locally
  const items = [...collections.value]
  const [moved] = items.splice(dragIndex.value, 1)
  items.splice(targetIndex, 0, moved)
  collections.value = items

  dragIndex.value = null
  dropIndex.value = null

  // Persist
  const orders = items.map((c, i) => ({ collection_id: c.id, display_order: i }))
  await reorderCollections(orders)
}

const onDragEnd = () => {
  dragIndex.value = null
  dropIndex.value = null
}

onMounted(loadCollections)
</script>

<template>
  <div class="flex flex-col gap-4">
    <!-- Section Header -->
    <div class="flex items-center justify-between gap-3 pb-4 border-b border-default">
      <div class="flex items-center gap-2">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-accent flex-shrink-0" viewBox="0 0 20 20" fill="currentColor">
          <path d="M7 3a1 1 0 000 2h6a1 1 0 100-2H7zM4 7a1 1 0 011-1h10a1 1 0 110 2H5a1 1 0 01-1-1zM2 11a2 2 0 012-2h12a2 2 0 012 2v4a2 2 0 01-2 2H4a2 2 0 01-2-2v-4z" />
        </svg>
        <h3 class="text-lg font-semibold text-primary">{{ $t('docs-collection-browser-heading') }}</h3>
      </div>
      <button
        @click="showCreateForm = !showCreateForm"
        class="flex items-center gap-1 text-xs px-3 py-1.5 rounded-md bg-surface-alt hover:bg-surface-hover text-secondary hover:text-primary transition-colors flex-shrink-0"
      >
        <Icon name="add" />
        {{ $t('docs-collection-browser-new') }}
      </button>
    </div>

    <!-- Create Form -->
    <form
      v-if="showCreateForm"
      @submit.prevent="handleCreate"
      class="flex flex-col gap-3 p-4 bg-surface-alt rounded-lg border border-default"
    >
      <div class="flex items-center gap-3">
        <DocumentIconSelector
          :initial-icon="newCollectionIcon"
          size="md"
          @update:icon="newCollectionIcon = $event"
        />
        <input
          v-model="newCollectionName"
          :placeholder="$t('docs-collection-browser-name-placeholder')"
          class="flex-1 min-w-0 h-10 bg-transparent text-sm text-primary placeholder:text-tertiary border border-subtle rounded-md px-3 focus:border-accent focus:outline-none"
          autofocus
        />
      </div>
      <div class="flex items-center justify-end gap-2">
        <button
          type="button"
          @click="showCreateForm = false"
          class="px-3 py-1.5 text-xs rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
        >
          {{ $t('docs-collection-browser-cancel') }}
        </button>
        <button
          type="submit"
          :disabled="!newCollectionName.trim()"
          class="px-4 py-1.5 text-xs rounded-md bg-accent text-on-accent hover:opacity-90 transition-opacity disabled:opacity-50"
        >
          {{ $t('docs-collection-browser-create') }}
        </button>
      </div>
    </form>

    <!--
      Loading skeleton — must mirror the real collection-card layout
      (icon + title + optional description + footer row) so the cards
      don't reshuffle when the real data lands.
    -->
    <Skeleton
      v-if="showSkeleton"
      :label="$t('docs-collection-browser-loading-label')"
      class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4"
    >
      <div
        v-for="i in skeletonCount"
        :key="i"
        class="flex flex-col gap-3 p-4 rounded-lg bg-surface border border-default"
      >
        <div class="flex items-start gap-3">
          <SkeletonBar class="w-7 h-7 rounded flex-shrink-0" />
          <div class="flex-1 flex flex-col gap-2">
            <SkeletonBar class="h-3.5 w-3/4" />
            <SkeletonBar class="h-3 w-full" />
          </div>
        </div>
        <div class="flex items-center justify-between pt-3 border-t border-subtle">
          <SkeletonBar class="h-3 w-16" />
          <SkeletonBar class="h-4 w-12 rounded" />
        </div>
      </div>
    </Skeleton>

    <!-- Collection Cards -->
    <div v-else-if="collections.length > 0" class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
      <RouterLink
        v-for="(collection, index) in collections"
        :key="collection.id"
        :to="`/documentation/collections/${collection.slug}`"
        :draggable="canReorder"
        @dragstart="canReorder && onDragStart(index, $event)"
        @dragover="canReorder && onDragOver(index, $event)"
        @dragleave="canReorder && onDragLeave()"
        @drop.prevent="canReorder && onDrop(index)"
        @dragend="canReorder && onDragEnd()"
        class="flex flex-col gap-3 p-4 rounded-lg bg-surface border border-default hover:border-accent transition-all"
        :class="{
          'opacity-50': dragIndex === index,
          'ring-2 ring-accent/40': dropIndex === index,
          'cursor-grab': canReorder,
          'cursor-pointer': !canReorder,
        }"
        :style="collection.color ? { '--card-accent': collection.color } : {}"
      >
        <div class="flex items-start gap-3">
          <span class="text-2xl leading-none flex-shrink-0">{{ collection.icon || '📁' }}</span>
          <div class="flex-1 min-w-0">
            <h4 class="font-medium text-primary text-sm truncate">{{ collection.name }}</h4>
            <p v-if="collection.description" class="text-xs text-tertiary mt-1 line-clamp-2">
              {{ collection.description }}
            </p>
          </div>
        </div>
        <div class="flex items-center justify-between pt-3 border-t border-subtle">
          <span class="text-xs text-tertiary">
            {{ $t('docs-collection-browser-pages', { count: collection.page_count }) }}
          </span>
          <div class="flex items-center gap-1.5">
            <span v-if="collection.is_system" class="text-[10px] px-1.5 py-0.5 rounded bg-surface-hover text-tertiary">{{ $t('docs-collection-browser-system-badge') }}</span>
            <span v-if="!collection.is_public" class="text-[10px] px-1.5 py-0.5 rounded bg-status-warning/10 text-status-warning">{{ $t('docs-collection-browser-restricted-badge') }}</span>
          </div>
        </div>
      </RouterLink>
    </div>

    <!-- Empty State -->
    <div v-else class="text-center py-8 text-tertiary text-sm">
      <p>{{ $t('docs-collection-browser-empty') }}</p>
    </div>

  </div>
</template>
