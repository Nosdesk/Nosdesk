<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { getCollections, setPageCollections } from '@/services/collectionService'
import type { CollectionWithDetails } from '@/services/collectionService'
import Icon from '@/components/common/Icon.vue'

const props = defineProps<{
  pageId: number
  currentCollectionIds: number[]
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'updated', collectionIds: number[]): void
}>()

const allCollections = ref<CollectionWithDetails[]>([])
const selectedIds = ref<Set<number>>(new Set(props.currentCollectionIds))
const loading = ref(true)
const saving = ref(false)

const hasChanges = computed(() => {
  if (selectedIds.value.size !== props.currentCollectionIds.length) return true
  return props.currentCollectionIds.some(id => !selectedIds.value.has(id))
})

const toggleCollection = (id: number) => {
  if (selectedIds.value.has(id)) {
    selectedIds.value.delete(id)
  } else {
    selectedIds.value.add(id)
  }
  // Force reactivity
  selectedIds.value = new Set(selectedIds.value)
}

const save = async () => {
  saving.value = true
  const ids = Array.from(selectedIds.value)
  const result = await setPageCollections(props.pageId, ids)
  saving.value = false
  if (result) {
    emit('updated', ids)
    emit('close')
  }
}

onMounted(async () => {
  allCollections.value = await getCollections()
  loading.value = false
})
</script>

<template>
  <!-- Backdrop -->
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40" @click.self="emit('close')">
    <div class="bg-surface border border-default rounded-xl shadow-2xl w-full max-w-md mx-4 max-h-[80vh] flex flex-col">
      <!-- Header -->
      <div class="flex items-center justify-between p-4 border-b border-default">
        <h3 class="text-sm font-semibold text-primary">Manage Collections</h3>
        <button
          @click="emit('close')"
          class="text-tertiary hover:text-primary p-1 rounded-md hover:bg-surface-hover transition-colors"
        >
          <Icon name="close" />
        </button>
      </div>

      <!-- Collection List -->
      <div class="flex-1 overflow-y-auto p-2">
        <div v-if="loading" class="space-y-2 p-2">
          <div v-for="i in 4" :key="i" class="h-10 rounded-lg bg-surface-alt animate-pulse"></div>
        </div>

        <div v-else-if="allCollections.length === 0" class="p-4 text-center text-tertiary text-sm">
          No collections available.
        </div>

        <div v-else class="space-y-1">
          <button
            v-for="collection in allCollections"
            :key="collection.id"
            @click="toggleCollection(collection.id)"
            class="w-full flex items-center gap-3 p-2.5 rounded-lg text-left transition-colors"
            :class="selectedIds.has(collection.id)
              ? 'bg-accent/10 border border-accent/30'
              : 'hover:bg-surface-hover border border-transparent'"
          >
            <!-- Checkbox -->
            <div
              class="w-4 h-4 rounded border flex-shrink-0 flex items-center justify-center transition-colors"
              :class="selectedIds.has(collection.id)
                ? 'bg-accent border-accent'
                : 'border-subtle'"
            >
              <span v-if="selectedIds.has(collection.id)" class="text-white inline-flex">
                <Icon name="check" size="xs" />
              </span>
            </div>

            <!-- Collection Info -->
            <span class="text-lg flex-shrink-0">{{ collection.icon || '📁' }}</span>
            <div class="flex-1 min-w-0">
              <span class="text-sm text-primary truncate block">{{ collection.name }}</span>
              <span class="text-xs text-tertiary">{{ collection.page_count }} pages</span>
            </div>

            <!-- System/Restricted badge -->
            <span v-if="collection.is_system" class="text-[10px] px-1.5 py-0.5 rounded bg-surface-hover text-tertiary flex-shrink-0">
              System
            </span>
          </button>
        </div>
      </div>

      <!-- Footer -->
      <div class="flex items-center justify-end gap-2 p-3 border-t border-default">
        <button
          @click="emit('close')"
          class="px-3 py-1.5 text-xs rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
        >
          Cancel
        </button>
        <button
          @click="save"
          :disabled="!hasChanges || saving"
          class="px-3 py-1.5 text-xs rounded-md bg-accent text-white hover:opacity-90 transition-opacity disabled:opacity-50"
        >
          {{ saving ? 'Saving...' : 'Save' }}
        </button>
      </div>
    </div>
  </div>
</template>
