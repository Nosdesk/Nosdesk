<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { getCollections, createCollection, reorderCollections } from '@/services/collectionService'
import type { CollectionWithDetails } from '@/services/collectionService'
import { useAuthStore } from '@/stores/auth'
import DocumentIconSelector from '@/components/DocumentIconSelector.vue'

const router = useRouter()
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

const loadCollections = async () => {
  loading.value = true
  collections.value = await getCollections()
  loading.value = false
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
        <h3 class="text-lg font-semibold text-primary">Collections</h3>
      </div>
      <button
        @click="showCreateForm = !showCreateForm"
        class="flex items-center gap-1 text-xs px-3 py-1.5 rounded-md bg-surface-alt hover:bg-surface-hover text-secondary hover:text-primary transition-colors flex-shrink-0"
      >
        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 4v16m8-8H4" />
        </svg>
        New
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
          placeholder="Collection name..."
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
          Cancel
        </button>
        <button
          type="submit"
          :disabled="!newCollectionName.trim()"
          class="px-4 py-1.5 text-xs rounded-md bg-accent text-white hover:opacity-90 transition-opacity disabled:opacity-50"
        >
          Create
        </button>
      </div>
    </form>

    <!-- Loading Skeleton -->
    <div v-if="loading" class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
      <div v-for="i in 3" :key="i" class="h-24 rounded-lg bg-surface-alt animate-pulse"></div>
    </div>

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
            {{ collection.page_count }} page{{ collection.page_count !== 1 ? 's' : '' }}
          </span>
          <div class="flex items-center gap-1.5">
            <span v-if="collection.is_system" class="text-[10px] px-1.5 py-0.5 rounded bg-surface-hover text-tertiary">System</span>
            <span v-if="!collection.is_public" class="text-[10px] px-1.5 py-0.5 rounded bg-status-warning/10 text-status-warning">Restricted</span>
          </div>
        </div>
      </RouterLink>
    </div>

    <!-- Empty State -->
    <div v-else class="text-center py-8 text-tertiary text-sm">
      <p>No collections yet.</p>
    </div>

  </div>
</template>
