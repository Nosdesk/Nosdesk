<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import documentationService from '@/services/documentationService'
import type { Page } from '@/services/documentationService'
import Icon from '@/components/common/Icon.vue'

const props = defineProps<{
  pageId: string | number
  currentParentId: string | number | null
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'moved'): void
}>()

const allPages = ref<Page[]>([])
const loading = ref(true)
const moving = ref(false)
const searchQuery = ref('')
const selectedParentId = ref<string | number | null>(props.currentParentId)

// Flatten page tree for display, tracking depth
interface FlatPage {
  id: string | number
  title: string
  icon: string | null
  depth: number
  disabled: boolean
}

// Get all descendant IDs of the current page (to prevent circular moves)
const getDescendantIds = (pages: Page[], targetId: string | number): Set<string> => {
  const ids = new Set<string>()
  const findAndCollect = (children: Page[]) => {
    for (const page of children) {
      if (String(page.id) === String(targetId)) {
        // Found the target, collect all its descendants
        const collectAll = (p: Page) => {
          ids.add(String(p.id))
          if (p.children) p.children.forEach(collectAll)
        }
        collectAll(page)
        return true
      }
      if (page.children && findAndCollect(page.children)) return true
    }
    return false
  }
  findAndCollect(pages)
  return ids
}

const flatPages = computed(() => {
  const result: FlatPage[] = []
  const disabledIds = getDescendantIds(allPages.value, props.pageId)
  const query = searchQuery.value.toLowerCase()

  const flatten = (pages: Page[], depth: number) => {
    for (const page of pages) {
      const matches = !query || page.title.toLowerCase().includes(query)
      const hasMatchingDescendant = !matches && page.children?.some(function check(p: Page): boolean {
        return p.title.toLowerCase().includes(query) || (p.children?.some(check) ?? false)
      })

      if (matches || hasMatchingDescendant) {
        result.push({
          id: page.id,
          title: page.title,
          icon: page.icon,
          depth,
          disabled: disabledIds.has(String(page.id)),
        })
      }

      if (page.children && page.children.length > 0) {
        flatten(page.children, depth + 1)
      }
    }
  }

  flatten(allPages.value, 0)
  return result
})

const isCurrentLocation = (id: string | number | null) => {
  return String(id) === String(props.currentParentId)
}

const selectDestination = (id: string | number | null) => {
  selectedParentId.value = id
}

const hasChanged = computed(() => {
  return String(selectedParentId.value) !== String(props.currentParentId)
})

const handleMove = async () => {
  if (!hasChanged.value) return
  moving.value = true
  try {
    await documentationService.movePage(props.pageId, selectedParentId.value, 0)
    emit('moved')
  } catch (error) {
    console.error('Failed to move page:', error)
  } finally {
    moving.value = false
  }
}

onMounted(async () => {
  allPages.value = await documentationService.getPages()
  loading.value = false
})
</script>

<template>
  <!-- Backdrop -->
  <div class="fixed inset-0 z-overlay flex items-center justify-center bg-black/40" @click.self="emit('close')">
    <div class="bg-surface border border-default rounded-xl shadow-2xl w-full max-w-md mx-4 max-h-[80vh] flex flex-col">
      <!-- Header -->
      <div class="flex items-center justify-between p-4 border-b border-default">
        <h3 class="text-sm font-semibold text-primary">Move Document</h3>
        <button
          @click="emit('close')"
          class="text-tertiary hover:text-primary p-1 rounded-md hover:bg-surface-hover transition-colors"
        >
          <Icon name="close" />
        </button>
      </div>

      <!-- Search. py-2 keeps the field tall enough that iOS's
           virtual keyboard doesn't cover the input after focus —
           the smaller py-1.5 left it under the threshold the OS
           uses for auto-scroll-into-view. -->
      <div class="p-3 border-b border-default">
        <input
          v-model="searchQuery"
          type="text"
          placeholder="Search pages..."
          class="w-full px-3 py-2 text-sm bg-surface-alt border border-default rounded-md text-primary placeholder-tertiary focus:outline-none focus:ring-1 focus:ring-accent/50"
        />
      </div>

      <!-- Page Tree -->
      <div class="flex-1 overflow-y-auto p-2">
        <div v-if="loading" class="flex flex-col gap-2 p-2">
          <div v-for="i in 5" :key="i" class="h-8 rounded-lg bg-surface-alt animate-pulse"></div>
        </div>

        <div v-else class="flex flex-col gap-0.5">
          <!-- Root level option -->
          <button
            @click="selectDestination(null)"
            class="w-full flex items-center gap-2 px-2.5 py-2 rounded-lg text-left text-sm transition-colors"
            :class="[
              selectedParentId === null
                ? 'bg-accent/10 border border-accent/30 text-primary'
                : 'hover:bg-surface-hover border border-transparent text-secondary',
              isCurrentLocation(null) && selectedParentId !== null ? 'opacity-60' : ''
            ]"
          >
            <svg class="w-4 h-4 flex-shrink-0 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" />
            </svg>
            <span class="flex-1">Root level (no parent)</span>
            <span v-if="isCurrentLocation(null)" class="text-[10px] px-1.5 py-0.5 rounded bg-surface-alt text-tertiary">Current</span>
          </button>

          <!-- Pages -->
          <button
            v-for="fp in flatPages"
            :key="fp.id"
            @click="!fp.disabled && selectDestination(fp.id)"
            class="w-full flex items-center gap-2 py-2 pr-2.5 rounded-lg text-left text-sm transition-colors"
            :class="[
              fp.disabled
                ? 'opacity-30 cursor-not-allowed'
                : String(selectedParentId) === String(fp.id)
                  ? 'bg-accent/10 border border-accent/30 text-primary'
                  : 'hover:bg-surface-hover border border-transparent text-secondary',
              isCurrentLocation(fp.id) && String(selectedParentId) !== String(fp.id) ? 'opacity-60' : ''
            ]"
            :disabled="fp.disabled"
            :style="{ paddingLeft: `${10 + fp.depth * 16}px` }"
          >
            <span class="text-sm flex-shrink-0">{{ fp.icon || '📄' }}</span>
            <span class="flex-1 truncate">{{ fp.title }}</span>
            <span v-if="isCurrentLocation(fp.id)" class="text-[10px] px-1.5 py-0.5 rounded bg-surface-alt text-tertiary flex-shrink-0">Current</span>
          </button>

          <!-- Empty state -->
          <div v-if="flatPages.length === 0 && !loading" class="p-4 text-center text-tertiary text-sm">
            {{ searchQuery ? 'No matching pages found.' : 'No pages available.' }}
          </div>
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
          @click="handleMove"
          :disabled="!hasChanged || moving"
          class="px-3 py-1.5 text-xs rounded-md bg-accent text-white hover:opacity-90 transition-opacity disabled:opacity-50"
        >
          {{ moving ? 'Moving...' : 'Move' }}
        </button>
      </div>
    </div>
  </div>
</template>
