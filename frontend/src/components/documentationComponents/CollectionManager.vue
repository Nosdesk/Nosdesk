<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { getCollections, setPageCollections } from '@nosdesk/core/services/collectionService'
import type { CollectionWithDetails } from '@nosdesk/core/services/collectionService'
import Icon from '@/components/common/Icon.vue'
import CollectionIcon from '@/components/documentationComponents/CollectionIcon.vue'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'

useFluent()

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
  <Modal :show="true" :title="$t('docs-collection-manager-title')" size="sm" @close="emit('close')">
    <!-- Collection List -->
    <div v-if="loading" class="flex flex-col gap-2">
      <div v-for="i in 4" :key="i" class="h-10 rounded-lg bg-surface-alt animate-pulse"></div>
    </div>

    <div v-else-if="allCollections.length === 0" class="p-4 text-center text-tertiary text-sm">
      {{ $t('docs-collection-manager-empty') }}
    </div>

    <div v-else class="flex flex-col gap-1">
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
        <CollectionIcon
          :icon="collection.icon"
          :color="collection.color"
          size="md"
        />
        <div class="flex-1 min-w-0">
          <span class="text-sm text-primary truncate block">{{ collection.name }}</span>
          <span class="text-xs text-tertiary">{{ $t('docs-collection-manager-pages', { count: collection.page_count }) }}</span>
        </div>

        <!-- System/Restricted badge -->
        <span v-if="collection.is_system" class="text-[10px] px-1.5 py-0.5 rounded bg-surface-hover text-tertiary flex-shrink-0">
          {{ $t('docs-collection-manager-system-badge') }}
        </span>
      </button>
    </div>

    <template #footer>
      <div class="flex items-center justify-end gap-2">
        <Button variant="ghost" size="sm" @click="emit('close')">
          {{ $t('docs-collection-manager-cancel') }}
        </Button>
        <Button size="sm" :loading="saving" :disabled="!hasChanges" @click="save">
          {{ $t('docs-collection-manager-save') }}
        </Button>
      </div>
    </template>
  </Modal>
</template>
