<script setup lang="ts">
import { ref, computed, watchEffect, onMounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { getCollectionBySlug, addPageToCollection, updateCollection, deleteCollection, getPageOverridesInCollection } from '@/services/collectionService'
import type { CollectionWithPages, CollectionPage, PageOverrideInfo } from '@/services/collectionService'
import documentationService from '@/services/documentationService'
import { docUrl } from '@/utils/docUrl'
import { docsEmitter } from '@/services/docsEmitter'
import { useAuthStore } from '@/stores/auth'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import { useSyncDocsStore } from '@/sync/stores/documentation'
import BackButton from '@/components/common/BackButton.vue'
import Icon from '@/components/common/Icon.vue'
import Spinner from '@/components/common/Spinner.vue'
import CollectionTreeList from '@/components/documentationComponents/CollectionTreeList.vue'
import DocumentIconSelector from '@/components/DocumentIconSelector.vue'
import CollectionVisibilityModal from '@/components/documentationComponents/CollectionVisibilityModal.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import CollaborativeEditor from '@/components/CollaborativeEditor.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const route = useRoute()
const router = useRouter()
const titleManager = useTitleManager()
const authStore = useAuthStore()
const docNavStore = useDocumentationNavStore()
const collection = ref<CollectionWithPages | null>(null)
const loading = ref(true)
const creating = ref(false)

// Pages + page count derive from the sync pool, so add / remove / rename
// / reorder reflect live without a refetch or discrete SSE listeners.
// The collection's own metadata (visibility, description_doc_id) stays on
// the REST fetch since it isn't part of the sync model.
const docs = useSyncDocsStore()
const collectionId = computed(() => collection.value?.id ?? null)
// Flat CollectionPage list for this collection from the pool;
// CollectionTreeList builds (and filters/sorts) the tree itself.
const pages = computed<CollectionPage[]>(() => {
  const id = collectionId.value
  if (id == null) return []
  return docs.allPages
    .filter((p) => p.collection_id === id)
    .map((p) => ({
      id: p.id,
      uuid: p.uuid,
      title: p.title,
      slug: p.slug,
      icon: p.icon,
      status: p.status,
      parent_id: p.parent_id,
      display_order: p.display_order,
      created_at: p.created_at,
      updated_at: p.updated_at,
    }))
})
const pageCount = computed(
  () => pages.value.filter((p) => p.status !== 'deleted' && p.status !== 'archived').length,
)

// Keep the collection's name + icon live from the pool (the rest of its
// metadata is REST-sourced); replaces the collection-updated listener.
watchEffect(() => {
  const id = collection.value?.id
  if (id == null) return
  const pc = docs.allCollections.find((c) => c.id === id)
  if (!pc || !collection.value) return
  if (pc.name !== collection.value.name) {
    collection.value.name = pc.name
    titleManager.setCustomTitle(pc.name)
  }
  if (pc.icon !== collection.value.icon) collection.value.icon = pc.icon
})

// Editor state
const editContent = ref('')

// Management state
const showVisibilityModal = ref(false)
const pageOverrides = ref<PageOverrideInfo[]>([])
const overridesExpanded = ref(false)

// Editor binds to the collection's own Yjs room, not a sentinel
// "main page". The backend resolves `collection-${id}` to the
// `documentation_collections.description_yjs` column via the
// existing collaboration WebSocket handler.
const docId = computed(() => collection.value?.description_doc_id ?? null)

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
    titleManager.setCustomTitle(t('collection-not-found-title'))
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
      title: t('collection-new-page-default-title'),
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

const deleteModalTitle = computed(() =>
  collection.value
    ? t('collection-delete-title', { name: collection.value.name })
    : t('collection-delete-title-fallback'),
)
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <!-- Header -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2">
        <BackButton fallbackRoute="/documentation" :label="$t('collection-back-to-documentation')" />
        <div class="flex-1"></div>

        <!-- Delete collection button (admin only, non-system) -->
        <button
          v-if="collection && authStore.isAdmin && !collection.is_system"
          @click="handleDelete"
          class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md border border-default text-status-danger hover:bg-status-danger/10 transition-colors"
        >
          <Icon name="trash" />
          <span class="hidden sm:inline">{{ $t('collection-action-delete') }}</span>
        </button>

        <!-- Manage Access button (admin only) -->
        <button
          v-if="collection && authStore.isAdmin"
          @click="showVisibilityModal = true"
          class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md border border-default text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
        >
          <Icon name="lock" />
          <span class="hidden sm:inline">{{ $t('collection-action-manage-access') }}</span>
        </button>

        <!-- Create page button -->
        <button
          v-if="collection"
          @click="createPageInCollection"
          :disabled="creating"
          class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md bg-accent text-on-accent hover:bg-accent/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Icon v-if="!creating" name="add" />
          <Spinner v-else />
          <span class="hidden sm:inline">{{ $t('collection-action-new-page') }}</span>
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
        <div class="flex flex-col gap-6">
          <div class="h-4 w-24 bg-surface-alt animate-pulse rounded"></div>
          <div class="flex flex-col gap-2">
            <div class="h-3 w-full bg-surface-alt animate-pulse rounded"></div>
            <div class="h-3 w-3/4 bg-surface-alt animate-pulse rounded"></div>
          </div>
          <div class="h-4 w-16 bg-surface-alt animate-pulse rounded mt-8"></div>
          <div class="flex flex-col gap-1">
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
        <p class="text-primary font-medium mb-1">{{ $t('collection-not-found-heading') }}</p>
        <p class="text-tertiary text-sm mb-4">{{ $t('collection-not-found-description') }}</p>
        <RouterLink to="/documentation" class="text-accent text-sm hover:underline">
          {{ $t('collection-back-to-documentation') }}
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
            <span v-if="collection.is_system" class="text-xs px-2 py-0.5 rounded-full bg-surface-alt text-tertiary">{{ $t('collection-badge-system') }}</span>
            <span v-if="!collection.is_public" class="text-xs px-2 py-0.5 rounded-full bg-status-warning/10 text-status-warning">{{ $t('collection-badge-restricted') }}</span>
            <span v-else class="text-xs px-2 py-0.5 rounded-full bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">{{ $t('collection-badge-public') }}</span>
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
            <Icon name="copyMd" class="text-tertiary" />
            <h2 class="text-sm font-semibold text-secondary uppercase tracking-wide">{{ $t('collection-overview-heading') }}</h2>
          </div>
          <div class="collection-editor-wrapper">
            <CollaborativeEditor
              v-model="editContent"
              :doc-id="docId"
              :hide-revision-history="true"
              :placeholder="$t('collection-overview-placeholder')"
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
            <Icon
              name="chevronRight"
              class="transition-transform"
              :class="overridesExpanded ? 'rotate-90' : ''"
            />
            <Icon name="lock" />
            <span>{{ $t('collection-overrides-summary', { count: pageOverrides.length }) }}</span>
          </button>

          <div v-if="overridesExpanded" class="flex flex-col gap-1.5 border-t border-status-warning/20 px-3 py-2">
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
              <Icon name="archive" class="text-tertiary" />
              <h2 class="text-sm font-semibold text-secondary uppercase tracking-wide">{{ $t('collection-pages-heading') }}</h2>
            </div>
            <span class="text-xs text-tertiary tabular-nums">
              {{ $t('collection-page-count', { count: pageCount }) }}
            </span>
          </div>
          <CollectionTreeList :pages="pages" :overridePageIds="overridePageIds" />
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
      :title="deleteModalTitle"
      :message="$t('collection-delete-message')"
      :confirm-label="$t('collection-delete-confirm')"
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
