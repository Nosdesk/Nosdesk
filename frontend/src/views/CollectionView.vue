<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useTitleManager } from '@/composables/useTitleManager'
import { getCollectionBySlug, addPageToCollection, updateCollection, deleteCollection, getPageOverridesInCollection } from '@/services/collectionService'
import type { CollectionWithPages, CollectionPage, PageOverrideInfo } from '@/services/collectionService'
import documentationService from '@/services/documentationService'
import { docUrl } from '@/utils/docUrl'
import { docsEmitter } from '@/services/docsEmitter'
import { useAuthStore } from '@/stores/auth'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import { useSSEListeners } from '@/composables/useSSEListeners'
import BackButton from '@/components/common/BackButton.vue'
import CollectionTreeList from '@/components/documentationComponents/CollectionTreeList.vue'
import DocumentIconSelector from '@/components/DocumentIconSelector.vue'
import CollectionVisibilityModal from '@/components/documentationComponents/CollectionVisibilityModal.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import CollaborativeEditor from '@/components/CollaborativeEditor.vue'

const route = useRoute()
const router = useRouter()
const titleManager = useTitleManager()
const authStore = useAuthStore()
const docNavStore = useDocumentationNavStore()
const collection = ref<CollectionWithPages | null>(null)
const loading = ref(true)
const creating = ref(false)

// Editor state
const editContent = ref('')

// Management state
const showVisibilityModal = ref(false)
const pageOverrides = ref<PageOverrideInfo[]>([])
const overridesExpanded = ref(false)

const docId = computed(() => {
  if (!collection.value?.root_page_id) return null
  return `doc-${collection.value.root_page_id}`
})

const overridePageIds = computed(() => {
  return new Set(pageOverrides.value.map(o => o.page_id))
})

const loadCollection = async () => {
  const slug = route.params.slug as string
  if (!slug) return

  loading.value = true
  collection.value = await getCollectionBySlug(slug)

  if (collection.value) {
    titleManager.setCustomTitle(collection.value.name)

    // Load page overrides for technician+ users
    if (authStore.isTechnician) {
      pageOverrides.value = await getPageOverridesInCollection(collection.value.id)
    }
  } else {
    titleManager.setCustomTitle('Collection Not Found')
  }

  loading.value = false

  // Open permissions modal if navigated with ?permissions=true
  if (route.query.permissions === 'true' && collection.value && authStore.isAdmin) {
    showVisibilityModal.value = true
  }
}

const handleIconChange = async (icon: string) => {
  if (!collection.value) return
  collection.value.icon = icon
  await updateCollection(collection.value.id, { icon })
}

const updateName = async (newName: string) => {
  if (!collection.value) return
  const name = newName.trim()
  if (!name || name === collection.value.name) return
  collection.value.name = name
  titleManager.setCustomTitle(name)
  await updateCollection(collection.value.id, { name })
}

const createPageInCollection = async () => {
  if (!collection.value || creating.value) return

  creating.value = true
  try {
    const newPage = await documentationService.createArticle({
      title: 'New Page',
      content: '',
      description: '',
      status: 'draft',
      icon: '📄',
    })

    if (newPage?.id) {
      await addPageToCollection(collection.value.id, Number(newPage.id))
      docsEmitter.emit('doc:created', { id: newPage.id })
      router.push(docUrl(newPage))
    }
  } catch (error) {
    console.error('Failed to create page in collection:', error)
  } finally {
    creating.value = false
  }
}

const onVisibilityUpdated = async () => {
  // Reload collection to get updated visibility
  await loadCollection()
}

const showDeleteConfirm = ref(false)

const handleDelete = () => {
  if (!collection.value) return
  showDeleteConfirm.value = true
}

const doDelete = async () => {
  showDeleteConfirm.value = false
  if (!collection.value) return
  const success = await deleteCollection(collection.value.id)
  if (success) {
    docNavStore.refreshPages()
    router.push('/documentation')
  }
}

onMounted(loadCollection)

watch(() => route.params.slug, loadCollection)

// Open permissions modal when query param is added while already on the page
watch(() => route.query.permissions, (val) => {
  if (val === 'true' && collection.value && authStore.isAdmin) {
    showVisibilityModal.value = true
  }
})

// SSE integration for real-time updates
const { on, debouncedReload } = useSSEListeners({ reload: loadCollection })

/** Set of page IDs in this collection (flat lookup for SSE filtering) */
const collectionPageIds = computed(() => {
  if (!collection.value) return new Set<number>()
  const ids = new Set<number>()
  const collect = (pages: CollectionPage[]) => {
    for (const p of pages) {
      ids.add(p.id)
      if (p.children) collect(p.children)
    }
  }
  collect(collection.value.pages)
  return ids
})

on('collection-updated', (data) => {
  if (!collection.value) return
  const event = data as { collection_id: number; field: string; value: unknown }
  if (event.collection_id !== collection.value.id) return
  if (event.field === 'name' && typeof event.value === 'string') {
    collection.value.name = event.value
    titleManager.setCustomTitle(event.value)
  } else if (event.field === 'icon' && typeof event.value === 'string') {
    collection.value.icon = event.value
  }
})

on('documentation-created', () => {
  if (!collection.value) return
  // Can't tell from the event if the page belongs to this collection
  // (collection membership is a join table), so reload conservatively
  debouncedReload()
})

on('documentation-updated', (data) => {
  if (!collection.value) return
  const event = data as { document_id: number; field: string; value: unknown }
  // Only process events for pages that belong to this collection
  if (!collectionPageIds.value.has(event.document_id)) return
  if (event.field === 'status') {
    debouncedReload()
    return
  }
  // Update page fields in place (title, icon)
  const page = collection.value.pages.find(p => p.id === event.document_id)
  if (page) {
    if (event.field === 'title' && typeof event.value === 'string') {
      page.title = event.value
    } else if (event.field === 'icon' && typeof event.value === 'string') {
      page.icon = event.value
    }
  }
})
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <!-- Header -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2">
        <BackButton fallbackRoute="/documentation" label="Back to Documentation" />
        <div class="flex-1"></div>

        <!-- Delete collection button (admin only, non-system) -->
        <button
          v-if="collection && authStore.isAdmin && !collection.is_system"
          @click="handleDelete"
          class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md border border-default text-status-danger hover:bg-status-danger/10 transition-colors"
        >
          <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
          </svg>
          <span class="hidden sm:inline">Delete</span>
        </button>

        <!-- Manage Access button (admin only) -->
        <button
          v-if="collection && authStore.isAdmin"
          @click="showVisibilityModal = true"
          class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md border border-default text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
        >
          <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
          </svg>
          <span class="hidden sm:inline">Manage Access</span>
        </button>

        <!-- Create page button -->
        <button
          v-if="collection"
          @click="createPageInCollection"
          :disabled="creating"
          class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md bg-accent text-white hover:bg-accent/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <svg v-if="!creating" class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
          </svg>
          <svg v-else class="w-3.5 h-3.5 animate-spin" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          <span class="hidden sm:inline">New Page</span>
        </button>
      </div>
    </div>

    <!-- Main Content -->
    <div class="flex flex-col flex-1 overflow-auto bg-gradient-to-b from-bg-app to-bg-surface items-center">
      <!-- Loading skeleton -->
      <div v-if="loading" class="w-full max-w-4xl mx-auto px-4 sm:px-6 lg:px-8 py-6 sm:py-8">
        <div class="flex items-start gap-3 mb-6">
          <div class="w-10 h-10 rounded-lg bg-surface-alt animate-pulse flex-shrink-0"></div>
          <div class="flex-1 min-w-0">
            <div class="h-7 w-48 bg-surface-alt animate-pulse rounded mb-3"></div>
            <div class="flex gap-2">
              <div class="h-5 w-16 bg-surface-alt animate-pulse rounded-full"></div>
              <div class="h-5 w-20 bg-surface-alt animate-pulse rounded-full"></div>
            </div>
          </div>
        </div>
        <div class="space-y-6">
          <div class="h-4 w-24 bg-surface-alt animate-pulse rounded"></div>
          <div class="space-y-2">
            <div class="h-3 w-full bg-surface-alt animate-pulse rounded"></div>
            <div class="h-3 w-3/4 bg-surface-alt animate-pulse rounded"></div>
          </div>
          <div class="h-4 w-16 bg-surface-alt animate-pulse rounded mt-8"></div>
          <div class="space-y-1">
            <div v-for="i in 5" :key="i" class="flex items-center gap-2.5 py-2 px-3">
              <div class="w-5 h-5 rounded-md bg-surface-alt animate-pulse"></div>
              <div class="flex-1 h-3.5 rounded bg-surface-alt animate-pulse" :style="{ maxWidth: `${35 + (i % 3) * 15}%`, animationDelay: `${i * 60}ms` }"></div>
            </div>
          </div>
        </div>
      </div>

      <!-- Not found -->
      <div v-else-if="!collection" class="text-center py-16 px-4">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-12 w-12 text-tertiary mx-auto mb-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
        </svg>
        <p class="text-primary font-medium mb-1">Collection not found</p>
        <p class="text-tertiary text-sm mb-4">This collection may have been moved or deleted.</p>
        <RouterLink to="/documentation" class="text-accent text-sm hover:underline">
          Back to Documentation
        </RouterLink>
      </div>

      <!-- Collection content -->
      <div v-else class="w-full max-w-4xl mx-auto px-4 sm:px-6 lg:px-8 py-6 sm:py-8 flex flex-col gap-6">
        <!-- Collection Header -->
        <div>
          <div class="flex items-start gap-3 mb-3">
            <DocumentIconSelector
              :initial-icon="collection.icon || '📁'"
              size="lg"
              @update:icon="handleIconChange"
            />
            <h1
              contenteditable="true"
              @blur="updateName(($event.target as HTMLElement).textContent || '')"
              @keydown.enter.prevent="($event.target as HTMLElement).blur()"
              class="text-2xl sm:text-3xl font-bold text-primary break-words leading-tight tracking-tight outline-none focus:ring-1 focus:ring-accent/30 rounded px-1 -mx-1 flex-1"
            >{{ collection.name }}</h1>
          </div>
          <div class="flex items-center gap-2 flex-wrap">
            <span v-if="collection.is_system" class="text-xs px-2 py-0.5 rounded-full bg-surface-alt text-tertiary">System</span>
            <span v-if="!collection.is_public" class="text-xs px-2 py-0.5 rounded-full bg-status-warning/10 text-status-warning">Restricted</span>
            <span v-else class="text-xs px-2 py-0.5 rounded-full bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">Public</span>
            <span
              v-for="group in collection.visible_to_groups"
              :key="'g-' + group.id"
              class="text-xs px-2 py-0.5 rounded-full bg-accent/10 text-accent"
            >
              {{ group.name }}
            </span>
            <span
              v-for="user in collection.visible_to_users"
              :key="'u-' + user.uuid"
              class="text-xs px-2 py-0.5 rounded-full bg-blue-500/10 text-blue-600 dark:text-blue-400"
            >
              {{ user.name }}
            </span>
          </div>
        </div>

        <!-- Overview Section -->
        <section v-if="docId">
          <div class="flex items-center gap-2 mb-3 pb-2 border-b border-default">
            <svg class="w-4 h-4 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
            </svg>
            <h2 class="text-sm font-semibold text-secondary uppercase tracking-wide">Overview</h2>
          </div>
          <div class="collection-editor-wrapper">
            <CollaborativeEditor
              v-model="editContent"
              :doc-id="docId"
              :hide-revision-history="true"
              placeholder="Write an overview for this collection..."
              class="w-full"
            />
          </div>
        </section>

        <!-- Page Overrides Section (technician+) -->
        <div
          v-if="pageOverrides.length > 0"
          class="border border-status-warning/20 bg-status-warning/5 rounded-lg overflow-hidden"
        >
          <button
            @click="overridesExpanded = !overridesExpanded"
            class="w-full flex items-center gap-2 px-3 py-2 text-xs font-medium text-status-warning hover:bg-status-warning/10 transition-colors"
          >
            <svg
              class="w-3.5 h-3.5 transition-transform"
              :class="overridesExpanded ? 'rotate-90' : ''"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
            </svg>
            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
            </svg>
            <span>{{ pageOverrides.length }} page{{ pageOverrides.length !== 1 ? 's' : '' }} with custom permissions</span>
          </button>

          <div v-if="overridesExpanded" class="border-t border-status-warning/20 px-3 py-2 space-y-1.5">
            <RouterLink
              v-for="override in pageOverrides"
              :key="override.page_id"
              :to="`/documentation/${override.page_id}`"
              class="flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-surface-hover transition-colors group"
            >
              <span class="text-sm flex-shrink-0">{{ override.page_icon || '📄' }}</span>
              <span class="text-xs text-primary truncate flex-1 group-hover:text-accent">{{ override.page_title }}</span>
              <span
                v-for="group in override.groups"
                :key="'g-' + group.id"
                class="text-[10px] px-1.5 py-0.5 rounded-full bg-accent/10 text-accent flex-shrink-0"
              >
                {{ group.name }}
              </span>
              <span
                v-for="user in (override.users || [])"
                :key="'u-' + user.uuid"
                class="text-[10px] px-1.5 py-0.5 rounded-full bg-blue-500/10 text-blue-600 dark:text-blue-400 flex-shrink-0"
              >
                {{ user.name }}
              </span>
            </RouterLink>
          </div>
        </div>

        <!-- Pages Section -->
        <section>
          <div class="flex items-center justify-between gap-2 mb-3 pb-2 border-b border-default">
            <div class="flex items-center gap-2">
              <svg class="w-4 h-4 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
              </svg>
              <h2 class="text-sm font-semibold text-secondary uppercase tracking-wide">Pages</h2>
            </div>
            <span class="text-xs text-tertiary tabular-nums">
              {{ collection.page_count }} page{{ collection.page_count !== 1 ? 's' : '' }}
            </span>
          </div>
          <CollectionTreeList :pages="collection.pages" :overridePageIds="overridePageIds" />
        </section>
      </div>
    </div>

    <!-- Visibility Modal -->
    <CollectionVisibilityModal
      v-if="showVisibilityModal && collection"
      :collectionId="collection.id"
      :currentGroupIds="collection.visible_to_groups.map(g => g.id)"
      :currentUsers="collection.visible_to_users || []"
      @close="showVisibilityModal = false"
      @updated="onVisibilityUpdated"
    />

    <ConfirmModal
      :show="showDeleteConfirm"
      variant="danger"
      :title="collection ? `Delete ${collection.name}?` : 'Delete collection?'"
      message="Pages in this collection will not be deleted."
      confirm-label="Delete"
      @confirm="doDelete"
      @close="showDeleteConfirm = false"
    />
  </div>
</template>

<style scoped>
/* Override the editor's internal min-heights for the collection overview context */
.collection-editor-wrapper :deep(.editor-wrapper) {
  min-height: auto;
}

.collection-editor-wrapper :deep(.editor-container) {
  min-height: auto;
}

.collection-editor-wrapper :deep(.ProseMirror) {
  min-height: 1.5em;
  padding: 0.75rem 1rem;
}

/* Remove the toolbar border-radius mismatch since there's no surrounding card */
.collection-editor-wrapper :deep(.editor-container) {
  border-radius: 0 0 0.5rem 0.5rem;
}
</style>
