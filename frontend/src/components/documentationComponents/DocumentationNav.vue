<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, reactive, computed, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import documentationService, { getStarredPages } from '@/services/documentationService'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import { useAuthStore } from '@/stores/auth'
import type { Page } from '@/services/documentationService'
import DocumentationNavItem from './DocumentationNavItem.vue'
import MoveDocumentModal from './MoveDocumentModal.vue'
import PagePermissionsModal from './PagePermissionsModal.vue'
import ContextMenu from '@/components/common/ContextMenu.vue'
import type { MenuItem } from '@/components/common/ContextMenu.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import { storeToRefs } from 'pinia'
import { useSSE } from '@/services/sseService'
import { getCollections, getCollection, updateCollection, deleteCollection } from '@/services/collectionService'
import type { CollectionWithDetails, CollectionPage } from '@/services/collectionService'
import { buildTreeFromFlat, sortByOrder, findInTree } from '@/utils/treeUtils'
import { docUrl } from '@/utils/docUrl'
import { useClipboard } from '@/composables/useClipboard'
import { docsEmitter } from '@/services/docsEmitter'
import type { NavPage } from '@/stores/documentationNav'

/** SSE payload for documentation-updated events */
interface DocumentationUpdateEvent {
  document_id: string | number;
  field: string;
  value: unknown;
  updated_by?: string;
}

defineEmits<{
  'search': [query: string];
}>();

const route = useRoute()
const router = useRouter()
const docNavStore = useDocumentationNavStore()

// SSE for real-time updates
const { addEventListener, removeEventListener } = useSSE()

// Handle SSE documentation updates - update sidebar reactively
const handleDocumentationUpdate = (event: unknown) => {
  const data = ((event as { data?: DocumentationUpdateEvent })?.data ?? event) as DocumentationUpdateEvent;
  const { field } = data;
  if (field !== 'title' && field !== 'icon') return;

  const value = data.value as string;
  docNavStore.updatePageField(data.document_id, field, value);

  // Also update in our collection page caches
  for (const [, pages] of Object.entries(collectionPages)) {
    const page = findInTree(pages, data.document_id);
    if (page) page[field] = value;
  }
  // Update starred pages if title/icon changed
  const starred = starredPages.value.find(p => p.page_id === data.document_id);
  if (starred) starred[field] = value;
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

// Context menu state
const authStore = useAuthStore()
const { copy } = useClipboard()
const contextMenuPage = ref<Page | null>(null)
const contextMenuPos = ref({ x: 0, y: 0 })
const showContextMenu = ref(false)
const contextMenuType = ref<'page' | 'collection'>('page')
const contextMenuCollection = ref<CollectionWithDetails | null>(null)
const pendingDeleteCollection = ref<CollectionWithDetails | null>(null)

async function doDeleteCollection() {
  const collection = pendingDeleteCollection.value
  pendingDeleteCollection.value = null
  if (!collection) return
  try {
    const success = await deleteCollection(collection.id)
    if (success) {
      await reloadSidebar()
      if (route.path === `/documentation/collections/${collection.slug}`) {
        router.push('/documentation')
      }
    }
  } catch (error) {
    console.error('Failed to delete collection:', error)
  }
}

// Modals triggered from context menu
const showMoveModal = ref(false)
const moveModalPageId = ref<string | number>('')
const moveModalParentId = ref<string | number | null>(null)
const showPermissionsModal = ref(false)
const permissionsModalPageId = ref(0)

// SVG icon paths (stroke-based, viewBox 0 0 24 24)
const icons = {
  openTab: 'M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25',
  link: 'M13.19 8.688a4.5 4.5 0 011.242 7.244l-4.5 4.5a4.5 4.5 0 01-6.364-6.364l1.757-1.757m13.35-.622l1.757-1.757a4.5 4.5 0 00-6.364-6.364l-4.5 4.5a4.5 4.5 0 001.242 7.244',
  star: 'M11.48 3.499a.562.562 0 011.04 0l2.125 5.111a.563.563 0 00.475.345l5.518.442c.499.04.701.663.321.988l-4.204 3.602a.563.563 0 00-.182.557l1.285 5.385a.562.562 0 01-.84.61l-4.725-2.885a.562.562 0 00-.586 0L6.982 20.54a.562.562 0 01-.84-.61l1.285-5.386a.562.562 0 00-.182-.557l-4.204-3.602a.562.562 0 01.321-.988l5.518-.442a.563.563 0 00.475-.345L11.48 3.5z',
  duplicate: 'M15.75 17.25v3.375c0 .621-.504 1.125-1.125 1.125h-9.75a1.125 1.125 0 01-1.125-1.125V7.875c0-.621.504-1.125 1.125-1.125H6.75a9.06 9.06 0 011.5.124m7.5 10.376h3.375c.621 0 1.125-.504 1.125-1.125V11.25c0-4.46-3.243-8.161-7.5-8.876a9.06 9.06 0 00-1.5-.124H9.375c-.621 0-1.125.504-1.125 1.125v3.5m7.5 10.375H9.375a1.125 1.125 0 01-1.125-1.125v-9.25m12 6.625v-1.875a3.375 3.375 0 00-3.375-3.375h-1.5a1.125 1.125 0 01-1.125-1.125v-1.5a3.375 3.375 0 00-3.375-3.375H9.75',
  move: 'M3.75 9.776c.112-.017.227-.026.344-.026h15.812c.117 0 .232.009.344.026m-16.5 0a2.25 2.25 0 00-1.883 2.542l.857 6a2.25 2.25 0 002.227 1.932H19.05a2.25 2.25 0 002.227-1.932l.857-6a2.25 2.25 0 00-1.883-2.542m-16.5 0V6A2.25 2.25 0 016 3.75h3.879a1.5 1.5 0 011.06.44l2.122 2.12a1.5 1.5 0 001.06.44H18A2.25 2.25 0 0120.25 9v.776',
  permissions: 'M16.5 10.5V6.75a4.5 4.5 0 10-9 0v3.75m-.75 11.25h10.5a2.25 2.25 0 002.25-2.25v-6.75a2.25 2.25 0 00-2.25-2.25H6.75a2.25 2.25 0 00-2.25 2.25v6.75a2.25 2.25 0 002.25 2.25z',
  exportMd: 'M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3',
  archive: 'M20.25 7.5l-.625 10.632a2.25 2.25 0 01-2.247 2.118H6.622a2.25 2.25 0 01-2.247-2.118L3.75 7.5M10 11.25h4M3.375 7.5h17.25c.621 0 1.125-.504 1.125-1.125v-1.5c0-.621-.504-1.125-1.125-1.125H3.375c-.621 0-1.125.504-1.125 1.125v1.5c0 .621.504 1.125 1.125 1.125z',
  trash: 'M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0',
  rename: 'M16.862 4.487l1.687-1.688a1.875 1.875 0 112.652 2.652L6.832 19.82a4.5 4.5 0 01-1.897 1.13l-2.685.8.8-2.685a4.5 4.5 0 011.13-1.897L16.863 4.487z',
}

// Build page context menu items dynamically (star state depends on current page)
const pageContextMenuItems = computed((): MenuItem[] => {
  const page = contextMenuPage.value
  const isStarred = page ? starredPages.value.some(sp => sp.page_id === Number(page.id)) : false
  const isArchived = page?.status === 'archived'

  const items: MenuItem[] = [
    { id: 'open-new-tab', label: 'Open in new tab', icon: icons.openTab },
    { id: 'copy-link', label: 'Copy link', icon: icons.link },
    { id: isStarred ? 'unstar' : 'star', label: isStarred ? 'Remove star' : 'Star', icon: icons.star, divider: true },
    { id: 'duplicate', label: 'Duplicate', icon: icons.duplicate },
    { id: 'move', label: 'Move to...', icon: icons.move },
    { id: 'export-md', label: 'Export Markdown', icon: icons.exportMd },
  ]

  if (authStore.isAdmin) {
    items.push({ id: 'permissions', label: 'Permissions', icon: icons.permissions, divider: true })
  }

  items.push({
    id: isArchived ? 'restore' : 'archive',
    label: isArchived ? 'Restore' : 'Archive',
    icon: icons.archive,
    divider: !authStore.isAdmin,
  })
  items.push({ id: 'delete', label: 'Move to Trash', icon: icons.trash, danger: true })

  return items
})

// Collection context menu items
const collectionContextMenuItems = computed((): MenuItem[] => {
  const collection = contextMenuCollection.value
  const items: MenuItem[] = [
    { id: 'col-rename', label: 'Rename', icon: icons.rename },
  ]
  if (authStore.isAdmin) {
    items.push({ id: 'col-permissions', label: 'Permissions', icon: icons.permissions })
  }
  if (authStore.isAdmin && collection && !collection.is_system) {
    items.push({ id: 'col-delete', label: 'Delete', icon: icons.trash, danger: true, divider: true })
  }
  return items
})

// Active context menu items depend on type
const activeContextMenuItems = computed(() =>
  contextMenuType.value === 'collection'
    ? collectionContextMenuItems.value
    : pageContextMenuItems.value
)

const handleNavContextMenu = (page: Page, pos: { x: number, y: number }) => {
  contextMenuType.value = 'page'
  contextMenuPage.value = page
  contextMenuCollection.value = null
  contextMenuPos.value = pos
  showContextMenu.value = true
}

const handleCollectionContextMenu = (collection: CollectionWithDetails, event: MouseEvent) => {
  event.preventDefault()
  event.stopPropagation()
  contextMenuType.value = 'collection'
  contextMenuCollection.value = collection
  contextMenuPage.value = null
  contextMenuPos.value = { x: event.clientX, y: event.clientY }
  showContextMenu.value = true
}

// Inline rename state for collections
const renamingCollectionId = ref<number | null>(null)
const renameInput = ref('')
const renameInputRef = ref<HTMLInputElement | null>(null)

const startCollectionRename = async (collection: CollectionWithDetails) => {
  renamingCollectionId.value = collection.id
  renameInput.value = collection.name
  await nextTick()
  renameInputRef.value?.focus()
  renameInputRef.value?.select()
}

const commitCollectionRename = async () => {
  const id = renamingCollectionId.value
  if (!id) return
  const trimmed = renameInput.value.trim()
  if (trimmed) {
    const col = collections.value.find(c => c.id === id)
    if (col && trimmed !== col.name) {
      col.name = trimmed
      await updateCollection(id, { name: trimmed })
    }
  }
  renamingCollectionId.value = null
}

const cancelCollectionRename = () => {
  renamingCollectionId.value = null
}

const handleContextMenuSelect = async (actionId: string) => {
  // ---- Collection actions ----
  if (actionId.startsWith('col-')) {
    const collection = contextMenuCollection.value
    if (!collection) return

    switch (actionId) {
      case 'col-rename':
        startCollectionRename(collection)
        break
      case 'col-permissions':
        router.push({ path: `/documentation/collections/${collection.slug}`, query: { permissions: 'true' } })
        break
      case 'col-delete':
        pendingDeleteCollection.value = collection
        break
    }
    return
  }

  // ---- Page actions ----
  const page = contextMenuPage.value
  if (!page) return

  const pageUrl = docUrl(page)

  switch (actionId) {
    case 'open-new-tab':
      window.open(pageUrl, '_blank')
      break

    case 'copy-link':
      await copy(`${window.location.origin}${pageUrl}`)
      break

    case 'star':
      try {
        const success = await documentationService.starPage(Number(page.id))
        if (success) {
          docNavStore.addStarredPage({
            page_id: Number(page.id),
            title: page.title,
            slug: page.slug,
            icon: page.icon,
            starred_at: new Date().toISOString(),
          })
        }
      } catch (error) {
        console.error('Failed to star page:', error)
      }
      break

    case 'unstar':
      try {
        const success = await documentationService.unstarPage(Number(page.id))
        if (success) {
          docNavStore.removeStarredPage(Number(page.id))
        }
      } catch (error) {
        console.error('Failed to unstar page:', error)
      }
      break

    case 'duplicate':
      try {
        const newPage = await documentationService.createArticle({
          title: `${page.title} (copy)`,
          content: '',
          description: '',
          status: 'draft',
          icon: page.icon || '📄',
        })
        if (newPage?.id) {
          docsEmitter.emit('doc:created', { id: newPage.id })
          docNavStore.refreshPages()
          router.push(docUrl(newPage))
        }
      } catch (error) {
        console.error('Failed to duplicate page:', error)
      }
      break

    case 'move':
      moveModalPageId.value = page.id
      moveModalParentId.value = page.parent_id
      showMoveModal.value = true
      break

    case 'permissions':
      permissionsModalPageId.value = Number(page.id)
      showPermissionsModal.value = true
      break

    case 'export-md':
      try {
        const blob = await documentationService.exportPageMarkdown(page.id)
        if (!blob) return
        const url = URL.createObjectURL(blob)
        const a = window.document.createElement('a')
        a.href = url
        a.download = (page.slug || String(page.id)) + '.md'
        window.document.body.appendChild(a)
        a.click()
        window.document.body.removeChild(a)
        URL.revokeObjectURL(url)
      } catch (error) {
        console.error('Failed to export page:', error)
      }
      break

    case 'archive':
      try {
        await documentationService.archivePage(page.id)
        docNavStore.refreshPages()
        if (route.path === pageUrl) {
          router.push('/documentation')
        }
      } catch (error) {
        console.error('Failed to archive page:', error)
      }
      break

    case 'restore':
      try {
        await documentationService.restorePage(page.id)
        docNavStore.refreshPages()
      } catch (error) {
        console.error('Failed to restore page:', error)
      }
      break

    case 'delete':
      try {
        await documentationService.deleteArticle(page.id)
        docsEmitter.emit('doc:deleted', { id: page.id })
        docNavStore.refreshPages()
        if (route.path === pageUrl) {
          router.push('/documentation')
        }
      } catch (error) {
        console.error('Failed to delete page:', error)
      }
      break
  }
}

const handlePageMoved = () => {
  showMoveModal.value = false
  docNavStore.refreshPages()
}

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
  const pageRoute = foundPage ? docUrl(foundPage) : `/documentation/${stringId}`

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
            :to="docUrl({ slug: sp.slug, id: sp.page_id })"
            class="group flex items-center py-1 pr-2 rounded text-xs cursor-pointer transition-all duration-150"
            :class="[
              route.path === docUrl({ slug: sp.slug, id: sp.page_id })
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
          @contextmenu.prevent="handleCollectionContextMenu(collection, $event)"
        >
          <!-- Indent spacing -->
          <span class="flex-shrink-0" style="width: 8px"></span>

          <!-- Icon / Expand Toggle. The chevron only appears when
               the collection has pages, since an empty collection
               with a dropdown affordance falsely implies content
               sits behind it. The icon stays as a static glyph in
               that case. -->
          <span
            class="flex-shrink-0 w-5 flex items-center justify-center"
            @click.stop="(collection.page_count ?? 0) > 0 ? toggleCollectionExpanded(collection.id) : null"
          >
            <template v-if="(collection.page_count ?? 0) > 0">
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
            </template>
            <span v-else class="text-sm leading-none">{{ collection.icon || '📁' }}</span>
          </span>

          <!-- Collection Name (inline rename or static) -->
          <input
            v-if="renamingCollectionId === collection.id"
            ref="renameInputRef"
            v-model="renameInput"
            class="flex-1 min-w-0 ml-1 bg-surface border border-default rounded px-1 py-0 text-xs text-primary outline-none focus:border-accent"
            @keydown.enter="commitCollectionRename"
            @keydown.escape="cancelCollectionRename"
            @blur="commitCollectionRename"
            @click.stop
          />
          <span v-else class="flex-1 truncate min-w-0 ml-1">{{ collection.name }}</span>

          <!-- Page Count. Suppressed at zero so an empty
               collection doesn't carry a "0" badge. -->
          <span
            v-if="renamingCollectionId !== collection.id && (collection.page_count ?? 0) > 0"
            class="flex-shrink-0 text-[10px] text-tertiary ml-1"
          >
            {{ collection.page_count }}
          </span>
        </div>

        <!-- Collection Pages (collapsible with smooth transition).
             Empty collections are gated above; we only mount the
             collapse section when there's something to reveal. -->
        <div
          v-if="(collection.page_count ?? 0) > 0"
          class="collapse-section"
          :class="{ 'is-expanded': collectionExpanded[collection.id] }"
        >
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
                  @context-menu="handleNavContextMenu"
                />
              </ul>
            </template>
          </div>
        </div>
      </div>

      <!-- Empty State -->
      <div v-if="collections.length === 0" class="px-4 py-8 text-center">
        <div class="text-tertiary text-sm">No documents yet</div>
      </div>
    </div>

    <!-- Context Menu -->
    <ContextMenu
      v-if="showContextMenu"
      :items="activeContextMenuItems"
      :x="contextMenuPos.x"
      :y="contextMenuPos.y"
      @select="handleContextMenuSelect"
      @close="showContextMenu = false"
    />

  </nav>

  <!-- Teleport modals to body so they escape nav overflow/stacking constraints -->
  <Teleport to="body">
    <MoveDocumentModal
      v-if="showMoveModal"
      :page-id="moveModalPageId"
      :current-parent-id="moveModalParentId"
      @close="showMoveModal = false"
      @moved="handlePageMoved"
    />
  </Teleport>

  <Teleport to="body">
    <PagePermissionsModal
      v-if="showPermissionsModal"
      :page-id="permissionsModalPageId"
      @close="showPermissionsModal = false"
      @updated="docNavStore.refreshPages()"
    />
  </Teleport>

  <ConfirmModal
    :show="pendingDeleteCollection !== null"
    variant="danger"
    :title="pendingDeleteCollection ? `Delete ${pendingDeleteCollection.name}?` : 'Delete collection?'"
    message="Pages in this collection will not be deleted."
    confirm-label="Delete"
    @confirm="doDeleteCollection"
    @close="pendingDeleteCollection = null"
  />
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
