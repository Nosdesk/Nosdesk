<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useFluent } from 'fluent-vue'
import { getPages } from '@/services/documentationService'
import type { Page } from '@/services/documentationService'
import Icon from '@/components/common/Icon.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{
  excludeUuid?: string
}>()

const emit = defineEmits<{
  (e: 'select', doc: { uuid: string; title: string }): void
  (e: 'close'): void
}>()

const searchQuery = ref('')
const allPages = ref<Page[]>([])
const loading = ref(true)

const flatPages = computed(() => {
  const result: Array<{ id: string | number; uuid: string; title: string; icon: string | null; depth: number }> = []

  function flatten(pages: Page[], depth = 0) {
    for (const page of pages) {
      if (page.uuid && page.uuid !== props.excludeUuid) {
        result.push({
          id: page.id,
          uuid: page.uuid,
          title: page.title,
          icon: page.icon,
          depth,
        })
      }
      if (page.children?.length) {
        flatten(page.children, depth + 1)
      }
    }
  }

  flatten(allPages.value)
  return result
})

const filteredPages = computed(() => {
  if (!searchQuery.value) return flatPages.value
  const q = searchQuery.value.toLowerCase()
  return flatPages.value.filter(p => p.title.toLowerCase().includes(q))
})

const selectDoc = (doc: { uuid: string; title: string }) => {
  emit('select', doc)
}

onMounted(async () => {
  allPages.value = await getPages()
  loading.value = false
})
</script>

<template>
  <div class="fixed inset-0 z-overlay flex items-center justify-center bg-black/40" @click.self="emit('close')">
    <div class="bg-surface border border-default rounded-xl shadow-2xl w-full max-w-md mx-4 max-h-[70vh] flex flex-col">
      <!-- Header -->
      <div class="flex items-center justify-between p-4 border-b border-default">
        <h3 class="text-sm font-semibold text-primary">{{ t('editor-doc-picker-title') }}</h3>
        <button
          @click="emit('close')"
          class="text-tertiary hover:text-primary p-1 rounded-md hover:bg-surface-hover transition-colors"
          :aria-label="t('editor-doc-picker-close')"
          :title="t('editor-doc-picker-close')"
        >
          <Icon name="close" />
        </button>
      </div>

      <!-- Search -->
      <div class="p-3 border-b border-default">
        <input
          v-model="searchQuery"
          :placeholder="t('editor-doc-picker-search-placeholder')"
          class="w-full bg-surface-alt text-sm text-primary placeholder:text-tertiary border border-subtle rounded-lg px-3 py-2 focus:border-accent focus:outline-none"
          autofocus
        />
      </div>

      <!-- List -->
      <div class="flex-1 overflow-y-auto p-2">
        <div v-if="loading" class="flex flex-col gap-2 p-2">
          <div v-for="i in 5" :key="i" class="h-8 rounded bg-surface-alt animate-pulse"></div>
        </div>

        <div v-else-if="filteredPages.length === 0" class="p-4 text-center text-tertiary text-sm">
          {{ t('editor-doc-picker-empty') }}
        </div>

        <div v-else class="flex flex-col gap-0.5">
          <button
            v-for="doc in filteredPages"
            :key="doc.uuid"
            @click="selectDoc({ uuid: doc.uuid, title: doc.title })"
            class="w-full flex items-center gap-2 px-3 py-2 rounded-lg text-left hover:bg-surface-hover transition-colors"
            :style="{ paddingLeft: `${12 + doc.depth * 16}px` }"
          >
            <span class="text-sm flex-shrink-0">{{ doc.icon || '📄' }}</span>
            <span class="text-sm text-primary truncate">{{ doc.title }}</span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
