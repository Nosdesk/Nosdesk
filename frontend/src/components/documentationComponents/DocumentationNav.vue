<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, reactive } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import documentationService, { getStarredPages } from '@/services/documentationService'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import type { Page } from '@/services/documentationService'
import DocumentationNavItem from './DocumentationNavItem.vue'
import { storeToRefs } from 'pinia'
import { useSSE } from '@/services/sseService'
import { getCollections, getCollection } from '@/services/collectionService'
import type { CollectionWithDetails, CollectionPage } from '@/services/collectionService'
import { buildTreeFromFlat, sortByOrder, findInTree } from '@/utils/treeUtils'

defineEmits<{
  'search': [query: string];
}>();

const route = useRoute()
const router = useRouter()
const docNavStore = useDocumentationNavStore()

// SSE for real-time updates
const { addEventListener, removeEventListener } = useSSE()

// Handle SSE documentation updates - update sidebar reactively
const handleDocumentationUpdate = (event: any) => {
  const data = event.data || event;

  if (data.field === 'title' || data.field === 'icon') {
    docNavStore.updatePageField(data.document_id, data.field, data.value);
    // Also update in our collection page caches
    for (const [, pages] of Object.entries(collectionPages)) {
      const page = findInTree(pages, data.document_id);
      if (page) {
        (page as any)[data.field] = data.value;
      }
    }
    // Update starred pages if title/icon changed
    const starred = starredPages.value.find(p => p.page_id === data.document_id);
    if (starred) {
      (starred as any)[data.field] = data.value;
    }
  }
};

// Handle SSE collection updates - update sidebar reactively
const handleCollectionUpdate = (event: any) => {
  const data = event.data || event;
  const col = collections.value.find(c => c.id === data.collection_id);
  if (col && data.field && data.value !== undefined) {
    (col as any)[data.field] = data.value;
  }
};

// Use store's reactive loading state
const { isLoading, starredPages, isStarredExpanded } = storeToRefs(docNavStore)

// Drag and Drop state
const draggedPageId = ref<string | number | null>(null);
const dropTargetId = ref<string | number | null>(null);
const dropPosition = ref<'above' | 'inside' | 'below' | null>(null);
const isDragging = ref(false);

// Single global drop indicator state
const navRef = ref<HTMLElement | null>(null);
const dropIndicatorY = ref<number | null>(null);
const dropIndicatorIndent = ref<number>(8);

// Collections data
const collections = ref<CollectionWithDetails[]>([])
// Per-collection expanded state (stored in localStorage)
const collectionExpanded = reactive<Record<number, boolean>>({})
// Per-collection loaded page trees (lazy loaded)
const collectionPages = reactive<Record<number, Page[]>>({})
// Track which collections have been loaded
const collectionLoaded = reactive<Record<number, boolean>>({})
// Loading state per collection
const collectionLoading = reactive<Record<number, boolean>>({})
// Map from page ID -> collection ID (for auto-expand on navigate)
const pageToCollectionMap = ref<Record<string, number>>({})
// Parent map for page hierarchy within collections
const pageParentMap = ref<Record<string, string | null>>({})

// Initial loading state
const initialLoading = ref(true)

// ============================================================================
// Collection loading
// ============================================================================

const loadCollections = async () => {
  collections.value = await getCollections()

  // Restore expansion state from localStorage
  for (const c of collections.value) {
    const stored = localStorage.getItem(`docNavCollectionExpanded_${c.id}`)
    collectionExpanded[c.id] = stored === 'true'
  }
}

const toggleCollectionExpanded = async (collectionId: number) => {
  const newState = !collectionExpanded[collectionId]
  collectionExpanded[collectionId] = newState
  localStorage.setItem(`docNavCollectionExpanded_${collectionId}`, String(newState))

  // Lazy-load pages on first expand
  if (newState && !collectionLoaded[collectionId]) {
    await loadCollectionPages(collectionId)
  }
}

// Mirrors handlePageClick: click navigates + expands, re-click toggles
const handleCollectionClick = async (collection: CollectionWithDetails) => {
  const collectionRoute = `/documentation/collections/${collection.slug}`

  if (route.path === collectionRoute) {
    await toggleCollectionExpanded(collection.id)
  } else {
    if (!collectionExpanded[collection.id]) {
      await toggleCollectionExpanded(collection.id)
    }
    router.push(collectionRoute)
  }
}

// ============================================================================
// Page loading per collection
// ============================================================================

// sortByOrder and buildTreeFromFlat imported from @/utils/treeUtils

const buildParentMapFromTree = (tree: Page[], parentId: string | null = null) => {
  for (const page of tree) {
    pageParentMap.value[String(page.id)] = parentId;
    if (page.children && page.children.length > 0) {
      buildParentMapFromTree(page.children, String(page.id));
    }
  }
};

const loadCollectionPages = async (collectionId: number) => {
  // Only show skeleton on first load — subsequent reloads keep showing
  // existing data (stale-while-revalidate) and update reactively when done
  const isFirstLoad = !collectionLoaded[collectionId];
  if (isFirstLoad) {
    collectionLoading[collectionId] = true;
  }
  try {
    const data = await getCollection(collectionId);
    if (data && data.pages) {
      const tree = buildTreeFromFlat(data.pages);
      collectionPages[collectionId] = tree;
      collectionLoaded[collectionId] = true;

      // Update page -> collection map
      for (const p of data.pages) {
        pageToCollectionMap.value[String(p.id)] = collectionId;
      }

      // Build parent map for these pages
      buildParentMapFromTree(tree);

      // Update the store pages for SSE reactivity
      updateStorePages();
    }
  } catch (error) {
    console.error(`Error loading pages for collection ${collectionId}:`, error);
  } finally {
    if (isFirstLoad) {
      collectionLoading[collectionId] = false;
    }
  }
};

// Sync all collection page trees into the docNavStore for SSE field updates
const updateStorePages = () => {
  const allPages: Page[] = [];
  for (const pages of Object.values(collectionPages)) {
    allPages.push(...pages);
  }
  docNavStore.setPages(allPages);
};

// ============================================================================
// Page navigation and interaction
// ============================================================================

const findParentPage = (tree: Page[], childId: string | number): Page | null => {
  for (const page of tree) {
    if (page.children && page.children.some(child => String(child.id) === String(childId))) {
      return page;
    }
    if (page.children && page.children.length > 0) {
      const found = findParentPage(page.children, childId);
      if (found) return found;
    }
  }
  return null;
};

// Find a page across all collections
const findPageGlobal = (id: string | number): Page | null => {
  for (const pages of Object.values(collectionPages)) {
    const found = findInTree(pages, id);
    if (found) return found;
  }
  return null;
};

// Get the page tree for a specific collection (used for drag-drop context)
const getPagesForCollection = (collectionId: number): Page[] => {
  return collectionPages[collectionId] || [];
};

const handlePageClick = (id: string | number) => {
  const stringId = String(id)
  const foundPage = findPageGlobal(id);
  const pageRoute = `/documentation/${foundPage?.slug || stringId}`

  if (foundPage && foundPage.children && foundPage.children.length > 0) {
    if (route.path === pageRoute) {
      docNavStore.togglePage(stringId)
    } else {
      docNavStore.expandPage(stringId)
    }
  }

  router.push(pageRoute)
}

const handleToggleExpand = (id: string | number) => {
  docNavStore.togglePage(String(id))
}

// Auto-expand the collection containing a page
const autoExpandForPage = async (pageId: string) => {
  const collectionId = pageToCollectionMap.value[pageId];

  if (collectionId !== undefined) {
    if (!collectionExpanded[collectionId]) {
      collectionExpanded[collectionId] = true;
      localStorage.setItem(`docNavCollectionExpanded_${collectionId}`, 'true');
    }
    if (!collectionLoaded[collectionId]) {
      await loadCollectionPages(collectionId);
    }

    // Also expand parent pages within the collection
    docNavStore.expandParents(pageId, pageParentMap.value);
    return;
  }

  // Page not yet in our map - it might be in an unloaded collection.
  // Load all collections to find it.
  for (const c of collections.value) {
    if (!collectionLoaded[c.id]) {
      await loadCollectionPages(c.id);
      if (pageToCollectionMap.value[pageId] !== undefined) {
        // Found it! Expand the collection.
        collectionExpanded[c.id] = true;
        localStorage.setItem(`docNavCollectionExpanded_${c.id}`, 'true');
        docNavStore.expandParents(pageId, pageParentMap.value);
        return;
      }
    }
  }

}

// Watch route changes to auto-expand pages
watch(() => route.path, (newPath) => {
  if (newPath.startsWith('/documentation/') && !newPath.includes('/collections/')) {
    const pageId = newPath.split('/').pop() || '';
    if (pageId) {
      autoExpandForPage(pageId);
    }
  }
})

// ============================================================================
// Drag and Drop (scoped within collections)
// ============================================================================

// Track which collection a drag operation is within
const dragCollectionId = ref<number | null>(null);

const handlePageDragStart = (id: string | number, event: DragEvent) => {
  draggedPageId.value = id;
  isDragging.value = true;
  // Remember which collection this page belongs to
  dragCollectionId.value = pageToCollectionMap.value[String(id)] ?? null;
};

const handlePageDragEnd = () => {
  isDragging.value = false;
  draggedPageId.value = null;
  dropTargetId.value = null;
  dropPosition.value = null;
  dropIndicatorY.value = null;
  dragCollectionId.value = null;
};

const getAllChildrenIds = (page: Page): string[] => {
  const ids: string[] = [];
  if (page.children && page.children.length > 0) {
    for (const child of page.children) {
      ids.push(String(child.id));
      ids.push(...getAllChildrenIds(child));
    }
  }
  return ids;
};

const wouldCreateCircularReference = (draggedId: string | number, targetId: string | number, position: 'above' | 'inside' | 'below'): boolean => {
  if (String(draggedId) === String(targetId)) return true;

  const draggedPage = findPageGlobal(draggedId);
  if (!draggedPage) return true;

  const descendantIds = getAllChildrenIds(draggedPage);
  return descendantIds.includes(String(targetId));
};

const handlePageDragOver = (id: string | number, event: DragEvent, position: 'above' | 'inside' | 'below', level: number = 0) => {
  // Only allow drops within the same collection
  const targetCollectionId = pageToCollectionMap.value[String(id)] ?? null;
  if (dragCollectionId.value !== null && targetCollectionId !== dragCollectionId.value) {
    dropTargetId.value = null;
    dropPosition.value = null;
    dropIndicatorY.value = null;
    return;
  }

  if (wouldCreateCircularReference(draggedPageId.value as string | number, id, position)) {
    dropTargetId.value = null;
    dropPosition.value = null;
    dropIndicatorY.value = null;
    return;
  }

  dropTargetId.value = id;
  dropPosition.value = position;

  if (position === 'above' || position === 'below') {
    const targetElement = event.currentTarget as HTMLElement;
    if (targetElement && navRef.value) {
      const targetRect = targetElement.getBoundingClientRect();
      const navRect = navRef.value.getBoundingClientRect();
      const yPos = position === 'above'
        ? targetRect.top - navRect.top
        : targetRect.bottom - navRect.top;
      dropIndicatorY.value = yPos;
      dropIndicatorIndent.value = 8 + (level * 8);
    }
  } else {
    dropIndicatorY.value = null;
  }
};

const handlePageDrop = async (id: string | number, event: DragEvent, position: 'above' | 'inside' | 'below') => {
  if (!draggedPageId.value || !position) return;

  // Ensure same-collection constraint
  const targetCollectionId = pageToCollectionMap.value[String(id)] ?? null;
  if (dragCollectionId.value !== null && targetCollectionId !== dragCollectionId.value) {
    handlePageDragEnd();
    return;
  }

  if (wouldCreateCircularReference(draggedPageId.value, id, position)) {
    handlePageDragEnd();
    return;
  }

  const collectionId = dragCollectionId.value;
  if (collectionId === null) {
    handlePageDragEnd();
    return;
  }

  const collectionTree = getPagesForCollection(collectionId);

  try {
    const targetPage = findInTree(collectionTree, id);
    if (!targetPage) return;

    const targetParent = findParentPage(collectionTree, id);
    const targetParentId = targetParent ? targetParent.id : null;

    if (position === 'inside') {
      await documentationService.movePage(draggedPageId.value, id, 0);
      docNavStore.expandPage(String(id));
    } else {
      const draggedPageCurrentParent = findParentPage(collectionTree, draggedPageId.value as string | number);
      const draggedPageCurrentParentId = draggedPageCurrentParent ? draggedPageCurrentParent.id : null;
      const needsParentChange = String(draggedPageCurrentParentId) !== String(targetParentId);

      let siblings: Page[] = [];
      if (targetParentId) {
        const parent = findInTree(collectionTree, targetParentId);
        if (parent && parent.children) siblings = [...parent.children];
      } else {
        siblings = [...collectionTree];
      }

      const targetIndex = siblings.findIndex(p => String(p.id) === String(id));
      if (targetIndex === -1) return;

      const newIndex = position === 'above' ? targetIndex : targetIndex + 1;

      if (needsParentChange) {
        await documentationService.movePage(draggedPageId.value, targetParentId, newIndex);

        const siblingsWithoutDragged = siblings.filter(p => String(p.id) !== String(draggedPageId.value));
        const pageOrders: { page_id: number, display_order: number }[] = [];
        let orderIndex = 0;

        for (let i = 0; i < siblingsWithoutDragged.length; i++) {
          if (orderIndex === newIndex) {
            pageOrders.push({ page_id: Number(draggedPageId.value), display_order: orderIndex });
            orderIndex++;
          }
          pageOrders.push({ page_id: Number(siblingsWithoutDragged[i].id), display_order: orderIndex });
          orderIndex++;
        }

        if (newIndex >= siblingsWithoutDragged.length) {
          pageOrders.push({ page_id: Number(draggedPageId.value), display_order: orderIndex });
        }

        await documentationService.reorderPages(targetParentId || null, pageOrders);
      } else {
        const pageOrders = siblings
          .filter(p => String(p.id) !== String(draggedPageId.value))
          .map((p, i) => {
            if (i >= newIndex) {
              return { page_id: Number(p.id), display_order: i + 1 };
            }
            return { page_id: Number(p.id), display_order: i };
          });

        pageOrders.splice(newIndex, 0, { page_id: Number(draggedPageId.value), display_order: newIndex });
        await documentationService.reorderPages(targetParentId || null, pageOrders);
      }
    }

    // Reload this collection's pages
    await loadCollectionPages(collectionId);
  } catch (error) {
    console.error('Error dropping page:', error);
  } finally {
    handlePageDragEnd();
  }
};

// ============================================================================
// Lifecycle
// ============================================================================

const handleResize = () => {
  docNavStore.updateSidebarForScreenSize()
}

onMounted(async () => {
  docNavStore.setLoading(true);
  try {
    // Load starred pages in parallel with collections
    getStarredPages().then(pages => docNavStore.setStarredPages(pages));
    await loadCollections()

    // Auto-expand collections that were previously expanded and load their pages
    const expandedCollectionLoads: Promise<void>[] = [];
    for (const c of collections.value) {
      if (collectionExpanded[c.id]) {
        expandedCollectionLoads.push(loadCollectionPages(c.id));
      }
    }
    await Promise.all(expandedCollectionLoads);

    // Auto-expand for current page
    const currentPath = route.path;
    if (currentPath.startsWith('/documentation/') && !currentPath.includes('/collections/')) {
      const currentPageId = currentPath.split('/').pop() || '';
      if (currentPageId) {
        await autoExpandForPage(currentPageId);

        // Expand current page if it has children
        const currentPage = findPageGlobal(currentPageId);
        if (currentPage && currentPage.children && currentPage.children.length > 0) {
          docNavStore.expandPage(currentPageId);
        }
      }
    }
  } finally {
    docNavStore.setLoading(false);
    initialLoading.value = false;
  }

  docNavStore.updateSidebarForScreenSize()
  window.addEventListener('resize', handleResize)
  addEventListener('documentation-updated' as any, handleDocumentationUpdate);
  addEventListener('collection-updated' as any, handleCollectionUpdate);
})

onUnmounted(() => {
  window.removeEventListener('resize', handleResize)
  removeEventListener('documentation-updated' as any, handleDocumentationUpdate);
  removeEventListener('collection-updated' as any, handleCollectionUpdate);
})

// Create a method to reload the sidebar
const reloadSidebar = async () => {
  getStarredPages().then(pages => docNavStore.setStarredPages(pages));
  await loadCollections();
  // Reload all expanded or previously loaded collections
  const reloads: Promise<void>[] = [];
  for (const c of collections.value) {
    if (collectionLoaded[c.id] || collectionExpanded[c.id]) {
      reloads.push(loadCollectionPages(c.id));
    }
  }
  await Promise.all(reloads);
};

defineExpose({ reloadSidebar });

// Watch for refresh requests from the store (counter-based, always fires on increment)
watch(() => docNavStore.needsRefresh, (newVal, oldVal) => {
  if (newVal > 0 && newVal !== oldVal) {
    reloadSidebar();
  }
});
</script>

<template>
  <nav ref="navRef" class="documentation-nav" :class="{ 'is-dragging': isDragging }">
    <!-- Single Global Drop Indicator -->
    <div
      v-if="dropIndicatorY !== null && isDragging"
      class="drop-indicator"
      :style="{
        top: `${dropIndicatorY}px`,
        left: `${dropIndicatorIndent}px`,
      }"
    >
      <div class="drop-indicator-dot"></div>
    </div>

    <!-- Loading State -->
    <div v-if="initialLoading" class="py-1 px-2">
      <div v-for="i in 3" :key="i" class="flex items-center gap-1.5 py-1">
        <div class="flex-shrink-0" style="width: 8px"></div>
        <div class="w-4 h-4 rounded bg-surface-hover/50 animate-pulse flex-shrink-0"></div>
        <div class="flex-1 h-3 rounded bg-surface-hover/60 animate-pulse" :style="{ maxWidth: `${50 + (i % 3) * 15}%` }"></div>
      </div>
    </div>

    <!-- Starred Pages Section -->
    <div v-if="!initialLoading && starredPages.length > 0" class="py-1">
      <!-- Starred Header -->
      <div
        class="group relative flex items-center py-1 pr-2 rounded text-xs cursor-pointer transition-all duration-150 text-secondary hover:text-primary hover:bg-surface-hover"
        @click="docNavStore.toggleStarredExpanded()"
      >
        <span class="flex-shrink-0" style="width: 8px"></span>
        <span class="flex-shrink-0 w-5 flex items-center justify-center">
          <svg
            class="w-2.5 h-2.5 transition-transform duration-200 text-tertiary"
            :class="{ 'rotate-90': isStarredExpanded }"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            stroke-width="2.5"
          >
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
          </svg>
        </span>
        <svg class="w-3.5 h-3.5 flex-shrink-0 ml-0.5 text-amber-500" fill="currentColor" viewBox="0 0 24 24">
          <path d="M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z" />
        </svg>
        <span class="flex-1 truncate min-w-0 ml-1 font-medium">Starred</span>
        <span class="flex-shrink-0 text-[10px] text-tertiary ml-1">{{ starredPages.length }}</span>
      </div>

      <!-- Starred Pages List (collapsible) -->
      <div class="collapse-section" :class="{ 'is-expanded': isStarredExpanded }">
        <div class="collapse-content">
          <RouterLink
            v-for="sp in starredPages"
            :key="sp.page_id"
            :to="`/documentation/${sp.slug}`"
            class="group flex items-center py-1 pr-2 rounded text-xs cursor-pointer transition-all duration-150"
            :class="[
              route.path === `/documentation/${sp.slug}`
                ? 'bg-surface text-primary font-medium'
                : 'text-secondary hover:text-primary hover:bg-surface-hover'
            ]"
          >
            <span class="flex-shrink-0" style="width: 20px"></span>
            <span class="flex-shrink-0 w-5 flex items-center justify-center">
              <span class="text-sm leading-none">{{ sp.icon || '📄' }}</span>
            </span>
            <span class="flex-1 truncate min-w-0 ml-1">{{ sp.title }}</span>
          </RouterLink>
        </div>
      </div>

      <!-- Divider between starred and collections -->
      <div class="my-1 mx-2 border-t border-subtle"></div>
    </div>

    <!-- Collection Folders -->
    <div v-if="!initialLoading" class="py-1">
      <!-- Each collection as an expandable folder -->
      <div v-for="collection in collections" :key="collection.id" class="collection-folder">
        <!-- Collection Header — same interaction pattern as DocumentationNavItem -->
        <div
          class="group relative flex items-center py-1 pr-2 rounded text-xs cursor-pointer transition-all duration-150"
          :class="[
            route.path === `/documentation/collections/${collection.slug}`
              ? 'bg-surface text-primary font-medium'
              : collectionExpanded[collection.id]
                ? 'text-primary hover:bg-surface-hover'
                : 'text-secondary hover:text-primary hover:bg-surface-hover'
          ]"
          @click.stop="handleCollectionClick(collection)"
        >
          <!-- Indent spacing -->
          <span class="flex-shrink-0" style="width: 8px"></span>

          <!-- Icon / Expand Toggle (arrow replaces icon on hover) -->
          <span
            class="flex-shrink-0 w-5 flex items-center justify-center"
            @click.stop="toggleCollectionExpanded(collection.id)"
          >
            <svg
              class="w-2.5 h-2.5 transition-transform duration-200 hidden group-hover:block text-tertiary"
              :class="{ 'rotate-90': collectionExpanded[collection.id] }"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="2.5"
            >
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
            </svg>
            <span class="text-sm leading-none group-hover:hidden">{{ collection.icon || '📁' }}</span>
          </span>

          <!-- Collection Name -->
          <span class="flex-1 truncate min-w-0 ml-1">{{ collection.name }}</span>

          <!-- Page Count -->
          <span class="flex-shrink-0 text-[10px] text-tertiary ml-1">{{ collection.page_count }}</span>
        </div>

        <!-- Collection Pages (collapsible with smooth transition) -->
        <div class="collapse-section" :class="{ 'is-expanded': collectionExpanded[collection.id] }">
          <div class="collapse-content">
            <template v-if="collectionExpanded[collection.id] || collectionLoaded[collection.id]">
              <!-- Loading state for this collection (first load only, sized to page_count) -->
              <div v-if="collectionLoading[collection.id]" class="py-0.5 ml-2">
                <div v-for="i in Math.max(1, Math.min(collection.page_count ?? 3, 8))" :key="i" class="flex items-center gap-1.5 py-1">
                  <div class="flex-shrink-0" :style="{ width: '20px' }"></div>
                  <div class="w-4 h-3.5 rounded bg-surface-hover/40 animate-pulse flex-shrink-0"></div>
                  <div class="flex-1 h-3 rounded bg-surface-hover/40 animate-pulse" :style="{ maxWidth: `${50 + (i % 3) * 15}%` }"></div>
                </div>
              </div>

              <!-- Pages tree -->
              <ul v-else-if="collectionPages[collection.id] && collectionPages[collection.id].length > 0" class="flex flex-col">
                <DocumentationNavItem
                  v-for="page in collectionPages[collection.id]"
                  :key="page.id"
                  :page="page"
                  :level="1"
                  :dragged-page-id="draggedPageId"
                  :is-dragging="String(draggedPageId) === String(page.id)"
                  :is-drop-target="String(dropTargetId) === String(page.id) && dropPosition === 'inside'"
                  @toggle-expand="handleToggleExpand"
                  @page-click="handlePageClick"
                  @drag-start="handlePageDragStart"
                  @drag-end="handlePageDragEnd"
                  @drag-over="(id, event, position, level) => handlePageDragOver(id, event, position, level)"
                  @drop="handlePageDrop"
                />
              </ul>

              <!-- Empty collection -->
              <div v-else-if="collectionLoaded[collection.id]" class="py-1 pl-8 text-[11px] text-tertiary italic">
                No pages
              </div>
            </template>
          </div>
        </div>
      </div>

      <!-- Empty State -->
      <div v-if="collections.length === 0" class="px-4 py-8 text-center">
        <div class="text-tertiary text-sm">No documents yet</div>
      </div>
    </div>
  </nav>
</template>

<style scoped>
.documentation-nav {
  position: relative;
}

.documentation-nav.is-dragging {
  user-select: none;
}

/* Smooth expand/collapse using CSS grid */
.collapse-section {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows 150ms ease-out;
}

.collapse-section.is-expanded {
  grid-template-rows: 1fr;
}

.collapse-content {
  overflow: hidden;
  min-height: 0;
}

/* Single global drop indicator */
.drop-indicator {
  position: absolute;
  right: 8px;
  height: 2px;
  background-color: #3b82f6;
  border-radius: 1px;
  pointer-events: none;
  z-index: 50;
  transform: translateY(-1px);
  transition: top 0.1s ease-out, left 0.1s ease-out;
}

.drop-indicator-dot {
  position: absolute;
  left: -3px;
  top: -3px;
  width: 8px;
  height: 8px;
  background-color: #3b82f6;
  border-radius: 50%;
}
</style>
