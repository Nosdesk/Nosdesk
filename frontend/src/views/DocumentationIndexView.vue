<script setup lang="ts">
import { ref, computed, onMounted, onActivated } from 'vue'
import { storeToRefs } from 'pinia'
import { useTitleManager } from '@/composables/useTitleManager'
import { useDocumentation } from '@/composables/useDocumentation'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import DocumentationCardGrid from '@/components/documentationComponents/DocumentationCardGrid.vue'
import DocumentationCardSkeleton from '@/components/documentationComponents/DocumentationCardSkeleton.vue'
import CollectionBrowser from '@/components/documentationComponents/CollectionBrowser.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import Icon from '@/components/common/Icon.vue'
import { getArchivedPages, getTrashedPages } from '@/services/documentationService'
import { getUncollectedPages } from '@/services/collectionService'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { docUrl } from '@/utils/docUrl'
import { formatRelativeTime } from '@/utils/dateUtils'
import type { Page } from '@/services/documentationService'

defineOptions({ name: 'DocumentationIndexView' })

const titleManager = useTitleManager()

const {
  pages,
  showSkeleton,
  loadAllPages,
  createNewPage,
} = useDocumentation()

const docNavStore = useDocumentationNavStore()
const { starredPages } = storeToRefs(docNavStore)

const draftCount = ref(0)
const archivedCount = ref(0)
const trashCount = ref(0)

const loadDraftCount = async () => {
  const drafts = await getUncollectedPages()
  draftCount.value = drafts.length
}
const loadArchivedCount = async () => {
  const archived = await getArchivedPages()
  archivedCount.value = archived.length
}
const loadTrashCount = async () => {
  const trashed = await getTrashedPages()
  trashCount.value = trashed.length
}

const handleCreatePage = async () => {
  try {
    await createNewPage()
  } catch (error) {
    console.error('Failed to create page:', error)
  }
}

function flattenTree(nodes: Page[]): Page[] {
  const out: Page[] = []
  for (const node of nodes) {
    out.push(node)
    if (node.children?.length) out.push(...flattenTree(node.children))
  }
  return out
}

const recentlyUpdated = computed<Page[]>(() => {
  const flat = flattenTree(pages.value)
  return flat
    .filter((p) => p.updated_at && p.status !== 'archived')
    .sort((a, b) => (b.updated_at ?? '').localeCompare(a.updated_at ?? ''))
    .slice(0, 8)
})

const visibleStarred = computed(() => starredPages.value.slice(0, 6))

const totalPages = computed(() => flattenTree(pages.value).length)

const hasStatusChips = computed(
  () => draftCount.value > 0 || archivedCount.value > 0 || trashCount.value > 0,
)

onMounted(async () => {
  titleManager.setCustomTitle('Documentation')
  await Promise.all([loadAllPages(), loadDraftCount(), loadArchivedCount(), loadTrashCount()])
})

onActivated(() => {
  loadAllPages()
  loadDraftCount()
  loadArchivedCount()
  loadTrashCount()
})

usePageCreateAction(handleCreatePage)
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <div class="flex flex-col flex-1 overflow-auto">
      <div class="flex flex-col max-w-7xl mx-auto w-full px-4 py-6 gap-8">

        <!--
          Page-level empty state for the first-run experience.
          When there are zero pages, the per-section "No pages yet"
          / "Star a page" copy reads as broken; replace the hub with
          a single guiding EmptyState that points at the create
          action. CollectionBrowser still shows below so an admin
          can set up collections before drafting.
        -->
        <EmptyState
          v-if="!showSkeleton && totalPages === 0"
          icon="document"
          :title="$t('empty-documentation-index-title')"
          :description="$t('empty-documentation-index-description')"
          action-label="New page"
          @action="handleCreatePage"
        />

        <!-- Hub: Recently updated + Starred -->
        <div v-else class="grid grid-cols-1 lg:grid-cols-3 gap-6">
          <!-- Recently updated -->
          <section class="lg:col-span-2 flex flex-col gap-3">
            <header class="flex items-center justify-between gap-3 pb-2 border-b border-default">
              <div class="flex items-center gap-2">
                <Icon name="history" class="text-accent" />
                <h2 class="text-sm font-semibold text-primary">Recently updated</h2>
              </div>
              <span v-if="!showSkeleton && recentlyUpdated.length > 0" class="text-[11px] text-tertiary">
                Last {{ recentlyUpdated.length }}
              </span>
            </header>

            <div v-if="showSkeleton" class="flex flex-col gap-1">
              <div v-for="i in 5" :key="i" class="flex items-center gap-2 py-1.5">
                <div class="w-4 h-4 rounded bg-surface-hover/60 animate-pulse" />
                <div class="flex-1 h-3 rounded bg-surface-hover/50 animate-pulse" :style="{ maxWidth: `${50 + (i % 3) * 12}%` }" />
                <div class="h-3 w-16 rounded bg-surface-hover/40 animate-pulse" />
              </div>
            </div>

            <ul v-else-if="recentlyUpdated.length > 0" class="flex flex-col">
              <li v-for="page in recentlyUpdated" :key="page.id">
                <RouterLink
                  :to="docUrl(page)"
                  class="group flex items-center gap-2 py-1.5 px-2 -mx-2 rounded text-sm text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
                >
                  <span class="flex-shrink-0 text-base leading-none">{{ page.icon || '📄' }}</span>
                  <span class="flex-1 truncate">{{ page.title }}</span>
                  <span class="flex-shrink-0 text-[11px] text-tertiary group-hover:text-secondary">
                    {{ formatRelativeTime(page.updated_at!) }}
                  </span>
                </RouterLink>
              </li>
            </ul>

            <!--
              Reachable when totalPages > 0 but every page is
              archived (recentlyUpdated filters status !== 'archived').
              The page-level EmptyState above covers the truly-empty
              first-run case.
            -->
            <p v-else class="text-sm text-tertiary py-4">No recent activity.</p>
          </section>

          <!-- Starred -->
          <section class="flex flex-col gap-3">
            <header class="flex items-center justify-between gap-3 pb-2 border-b border-default">
              <div class="flex items-center gap-2">
                <Icon name="star" class="text-amber-500" />
                <h2 class="text-sm font-semibold text-primary">Starred</h2>
              </div>
              <span v-if="visibleStarred.length > 0" class="text-[11px] text-tertiary">
                {{ starredPages.length }}
              </span>
            </header>

            <ul v-if="visibleStarred.length > 0" class="flex flex-col">
              <li v-for="sp in visibleStarred" :key="sp.page_id">
                <RouterLink
                  :to="docUrl({ slug: sp.slug, id: sp.page_id })"
                  class="flex items-center gap-2 py-1.5 px-2 -mx-2 rounded text-sm text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
                >
                  <span class="flex-shrink-0 text-base leading-none">{{ sp.icon || '📄' }}</span>
                  <span class="flex-1 truncate">{{ sp.title }}</span>
                </RouterLink>
              </li>
            </ul>

            <p v-else class="text-sm text-tertiary py-4">
              Star a page from its row menu for quick access.
            </p>
          </section>
        </div>

        <!-- Collections -->
        <CollectionBrowser />

        <!--
          Status chips. Three states (drafts, archived, trash) compressed
          into a single row instead of three full-width banners. Hidden
          entirely when all counts are zero.
        -->
        <div v-if="hasStatusChips" class="flex flex-wrap items-center gap-2">
          <RouterLink
            v-if="draftCount > 0"
            to="/documentation/drafts"
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-surface-alt hover:bg-surface-hover border border-default text-xs text-secondary hover:text-primary transition-colors"
          >
            <span>✏️</span>
            <span><span class="font-medium text-primary">{{ draftCount }}</span> draft{{ draftCount !== 1 ? 's' : '' }}</span>
          </RouterLink>

          <RouterLink
            v-if="archivedCount > 0"
            to="/documentation/archived"
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-surface-alt hover:bg-surface-hover border border-default text-xs text-secondary hover:text-primary transition-colors"
          >
            <Icon name="archive" class="text-tertiary" />
            <span><span class="font-medium text-primary">{{ archivedCount }}</span> archived</span>
          </RouterLink>

          <RouterLink
            v-if="trashCount > 0"
            to="/documentation/trash"
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-surface-alt hover:bg-surface-hover border border-default text-xs text-secondary hover:text-primary transition-colors"
          >
            <Icon name="trash" class="text-status-error/70" />
            <span><span class="font-medium text-primary">{{ trashCount }}</span> in trash</span>
          </RouterLink>
        </div>

        <!--
          Browse all. Demoted from a primary section to a native
          collapsible — useful when you genuinely want to scan the full
          set, hidden by default since the hub above covers most landings.
        -->
        <details v-if="!showSkeleton && totalPages > 0" class="docs-browse-all group">
          <summary class="flex items-center gap-2 py-2 cursor-pointer text-sm text-secondary hover:text-primary select-none">
            <Icon name="chevronRight" size="xs" class="text-tertiary transition-transform duration-200 group-open:rotate-90" />
            <span>Browse all pages</span>
            <span class="text-tertiary">({{ totalPages }})</span>
          </summary>
          <div class="pt-4">
            <DocumentationCardGrid :pages="pages" @create="handleCreatePage" />
          </div>
        </details>

        <DocumentationCardSkeleton v-else-if="showSkeleton" :count="6" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.docs-browse-all > summary {
  list-style: none;
}
.docs-browse-all > summary::-webkit-details-marker {
  display: none;
}
</style>
