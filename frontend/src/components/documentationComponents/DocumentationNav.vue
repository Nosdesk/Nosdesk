<script setup lang="ts">
import { ref, watch, watchEffect, onMounted, onUnmounted, reactive, computed, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import documentationService, { getStarredPages, createArticle } from '@/services/documentationService'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import { useAuthStore } from '@/stores/auth'
import type { Page } from '@/services/documentationService'
import DocumentationNavItem from './DocumentationNavItem.vue'
import NavRowActions from './NavRowActions.vue'
import CollectionModal from './CollectionModal.vue'
import CollectionIcon from './CollectionIcon.vue'
import Icon from '@/components/common/Icon.vue'
import { useLongPress } from '@/composables/useLongPress'
import { useDocumentPanelState } from '@/composables/useDocumentPanelState'
import MoveDocumentModal from './MoveDocumentModal.vue'
import PagePermissionsModal from './PagePermissionsModal.vue'
import ContextMenu from '@/components/common/ContextMenu.vue'
import type { MenuItem } from '@/components/common/ContextMenu.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import { storeToRefs } from 'pinia'
import { updateCollection, deleteCollection } from '@/services/collectionService'
import type { CollectionWithDetails } from '@/services/collectionService'
import { findInTree } from '@/utils/treeUtils'
import { docUrl } from '@nosdesk/core/utils/docUrl'
import { subscribe } from '@/sync/lifecycle'
import { useDelayedFlag } from '@/composables/useDelayedFlag'
import { useClipboard } from '@/composables/useClipboard'
import { docsEmitter } from '@/services/docsEmitter'
import type { NavPage } from '@/stores/documentationNav'
import {
  useSyncDocsStore,
  buildPageTree,
  isActivePage,
  type DocCollectionRow,
} from '@/sync/stores/documentation'
import { nodeToPage } from '@/composables/useDocPages'

const route = useRoute()
const router = useRouter()
const docNavStore = useDocumentationNavStore()
const docPanel = useDocumentPanelState()
const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

// Collections + per-collection page trees are sourced from the sync
// pool (see the watchEffect below), so there are no discrete
// documentation-updated / collection-updated listeners here: live
// edits flow in through the pool.

// Use store's reactive loading state
const { starredPages, isStarredExpanded } = storeToRefs(docNavStore)

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

/** Patch the in-memory collection list with the saved values
 * from the Edit modal so the sidebar reflects the change without
 * a full sidebar reload. */
function handleCollectionEdited(updated: CollectionWithDetails) {
  const idx = collections.value.findIndex((c) => c.id === updated.id)
  if (idx >= 0) {
    collections.value[idx] = { ...collections.value[idx], ...updated }
  }
}

// Modals triggered from context menu
const showMoveModal = ref(false)
const moveModalPageId = ref<string | number>('')
const moveModalParentId = ref<string | number | null>(null)
const showPermissionsModal = ref(false)
const permissionsModalPageId = ref(0)

// SVG icon paths (stroke-based, viewBox 0 0 24 24)
// Icon paths come from the central `common/icons.ts` registry so
// the same action carries the same glyph everywhere in the app.
// The `MenuItem.icon` field still wants a raw SVG path string for
// backward compatibility, so we map registry names to their `d`
// strings here. Action items where an icon doesn't aid recognition
// (Open in new tab, Move, Copy as plain text, etc.) are
// intentionally label-only — per Tonsky's "if everything has an
// icon, nothing stands out".
import { ICON_REGISTRY } from '@/components/common/icons'
const icons = {
  link: ICON_REGISTRY.link.d,
  search: ICON_REGISTRY.search.d,
  star: ICON_REGISTRY.star.d,
  duplicate: ICON_REGISTRY.duplicate.d,
  permissions: ICON_REGISTRY.lock.d,
  archive: ICON_REGISTRY.archive.d,
  trash: ICON_REGISTRY.trash.d,
  rename: ICON_REGISTRY.rename.d,
  add: ICON_REGISTRY.add.d,
  bell: ICON_REGISTRY.bell.d,
  history: ICON_REGISTRY.history.d,
  insights: ICON_REGISTRY.insights.d,
  copyMd: ICON_REGISTRY.copyMd.d,
  download: ICON_REGISTRY.download.d,
  print: ICON_REGISTRY.print.d,
}

// Build page context menu items dynamically (star state depends on current page)
const pageContextMenuItems = computed((): MenuItem[] => {
  const page = contextMenuPage.value
  const isStarred = page ? starredPages.value.some(sp => sp.page_id === Number(page.id)) : false
  const isArchived = page?.status === 'archived'

  // Icons appear only where they meaningfully aid scanning:
  //   - Star / Subscribe: status toggles, the icon doubles as a
  //     visual indicator of the toggle state in nearby UI
  //   - Add child: the universal `+`
  //   - Copy link / Copy as Markdown: paired actions where the
  //     icon distinguishes the destination format at a glance
  //   - Insights / History: feature actions that have established
  //     glyph metaphors (chart, clock-rewind)
  //   - Archive / Restore (paired): same icon, label flips
  //   - Trash: destructive — universal recognition
  //
  // Other items are label-only by intent: a row of identical
  // generic icons would have nothing to scan against.
  const items: MenuItem[] = [
    { id: 'open-new-tab', label: t('docs-nav-menu-open-new-tab') },
    { id: 'copy-link', label: t('docs-nav-menu-copy-link'), icon: icons.link },
    { id: 'copy-md', label: t('docs-nav-menu-copy-md'), icon: icons.copyMd },
    { id: 'copy-text', label: t('docs-nav-menu-copy-text'), divider: true },
    { id: 'add-child', label: t('docs-nav-menu-add-child'), icon: icons.add },
    { id: isStarred ? 'unstar' : 'star', label: isStarred ? t('docs-nav-menu-unstar') : t('docs-nav-menu-star'), icon: icons.star },
    { id: 'subscribe', label: t('docs-nav-menu-subscribe'), icon: icons.bell },
    { id: 'duplicate', label: t('docs-nav-menu-duplicate') },
    { id: 'move', label: t('docs-nav-menu-move'), divider: true },
    { id: 'history', label: t('docs-nav-menu-history'), icon: icons.history },
    { id: 'insights', label: t('docs-nav-menu-insights'), icon: icons.insights },
    { id: 'export-md', label: t('docs-nav-menu-export-md'), icon: icons.download },
    { id: 'print', label: t('docs-nav-menu-print') },
  ]

  if (authStore.isAdmin) {
    items.push({ id: 'permissions', label: t('docs-nav-menu-permissions'), icon: icons.permissions, divider: true })
  }

  items.push({
    id: isArchived ? 'restore' : 'archive',
    label: isArchived ? t('docs-nav-menu-restore') : t('docs-nav-menu-archive'),
    icon: icons.archive,
    divider: !authStore.isAdmin,
  })
  items.push({ id: 'delete', label: t('docs-nav-menu-trash'), icon: icons.trash, danger: true })

  return items
})

// Collection context menu items. Same iconography rule applies:
// reserve glyphs for actions where the visual aids recognition.
// Sort modes render as an inline radio group (heading + three
// items, check on the active one) instead of a cycle: the
// available options are visible at a glance and one click sets
// the mode, no nested submenu.
const collectionContextMenuItems = computed((): MenuItem[] => {
  const collection = contextMenuCollection.value
  const active = currentSortMode(collection?.id)
  const items: MenuItem[] = [
    { id: 'col-edit', label: t('docs-nav-col-edit'), icon: icons.rename },
    { id: 'col-sort-heading', label: t('docs-nav-col-sort-heading'), heading: true, divider: true },
    { id: 'col-sort-manual', label: SORT_LABELS.value.manual, checked: active === 'manual' },
    { id: 'col-sort-alpha', label: SORT_LABELS.value.alpha, checked: active === 'alpha' },
    { id: 'col-sort-recent', label: SORT_LABELS.value.recent, checked: active === 'recent' },
  ]
  if (authStore.isAdmin) {
    items.push({ id: 'col-permissions', label: t('docs-nav-col-permissions'), icon: icons.permissions, divider: true })
  }
  if (authStore.isAdmin && collection && !collection.is_system) {
    items.push({ id: 'col-delete', label: t('docs-nav-col-delete'), icon: icons.trash, danger: true, divider: true })
  }
  return items
})

// Active context menu items depend on type
const activeContextMenuItems = computed(() =>
  contextMenuType.value === 'collection'
    ? collectionContextMenuItems.value
    : pageContextMenuItems.value
)

/**
 * Single open-or-toggle entry point for every menu trigger
 * (right-click, `…` button click, long-press). The behaviour:
 *   - menu closed → open at the new anchor
 *   - menu open on the same target → close (toggle)
 *   - menu open on a different target → move to the new anchor
 *
 * Putting the policy in one place keeps every trigger in sync
 * with platform conventions (Notion / Linear / Slack / iOS all
 * toggle on second tap of the same trigger) without
 * special-casing each call site.
 */
function openOrToggleMenu(
  type: 'page' | 'collection',
  target: Page | CollectionWithDetails,
  pos: { x: number; y: number },
) {
  const sameAsActive =
    showContextMenu.value &&
    contextMenuType.value === type &&
    (type === 'page'
      ? contextMenuPage.value?.id === (target as Page).id
      : contextMenuCollection.value?.id === (target as CollectionWithDetails).id)
  if (sameAsActive) {
    showContextMenu.value = false
    return
  }
  contextMenuType.value = type
  if (type === 'page') {
    contextMenuPage.value = target as Page
    contextMenuCollection.value = null
  } else {
    contextMenuCollection.value = target as CollectionWithDetails
    contextMenuPage.value = null
  }
  contextMenuPos.value = pos
  showContextMenu.value = true
}

const handleNavContextMenu = (page: Page, pos: { x: number; y: number }) => {
  openOrToggleMenu('page', page, pos)
}

const handleCollectionContextMenu = (collection: CollectionWithDetails, event: MouseEvent) => {
  event.preventDefault()
  event.stopPropagation()
  openOrToggleMenu('collection', collection, { x: event.clientX, y: event.clientY })
}

/** Click-handler equivalent of the right-click context menu.
 * Anchors the menu to the bottom-left corner of the trigger
 * button so it visually grows out of the affordance instead of
 * appearing at the cursor. */
const openCollectionMenu = (collection: CollectionWithDetails, event: MouseEvent) => {
  const target = event.currentTarget as HTMLElement | null
  const rect = target?.getBoundingClientRect()
  const pos = rect
    ? { x: rect.left, y: rect.bottom + 4 }
    : { x: event.clientX, y: event.clientY }
  openOrToggleMenu('collection', collection, pos)
}

/** Touch UI long-press on a collection row opens the same
 * context menu desktop users reach via right-click. One
 * composable instance is shared across all rows; we capture
 * which row is being pressed via a ref set on pointerdown. The
 * single-instance pattern keeps the composable's cleanup scope
 * stable (per-iteration `useLongPress` calls inside the v-for
 * would orphan their `onScopeDispose` hooks). */
const pressedCollection = ref<CollectionWithDetails | null>(null)
const collectionLongPress = useLongPress((event) => {
  const collection = pressedCollection.value
  if (!collection) return
  openOrToggleMenu('collection', collection, { x: event.clientX, y: event.clientY })
  pressedCollection.value = null
})

function onCollectionPointerdown(collection: CollectionWithDetails, event: PointerEvent) {
  pressedCollection.value = collection
  collectionLongPress.listeners.pointerdown(event)
}

// Inline rename state for collections (legacy — preserved while
// the new "Edit collection" modal rolls out, both surfaces still
// commit through `updateCollection`).
const renamingCollectionId = ref<number | null>(null)
const renameInput = ref('')
const renameInputRef = ref<HTMLInputElement | null>(null)

// Edit-collection modal target. Null = closed; a collection row
// = the modal is mounted bound to that collection's fields.
const editingCollection = ref<CollectionWithDetails | null>(null)

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
      case 'col-edit':
        editingCollection.value = collection
        break
      case 'col-sort-manual':
        setSortMode(collection.id, 'manual')
        break
      case 'col-sort-alpha':
        setSortMode(collection.id, 'alpha')
        break
      case 'col-sort-recent':
        setSortMode(collection.id, 'recent')
        break
      case 'col-permissions':
        router.push({ path: `/documentation/collections/${collection.slug}`, query: { permissions: 'true' } })
        break
      case 'col-delete':
        pendingDeleteCollection.value = collection
        break
      // Legacy id kept for backwards compatibility while the menu
      // transitions to the richer Edit modal.
      case 'col-rename':
        startCollectionRename(collection)
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
          title: t('docs-nav-duplicate-suffix', { title: page.title }),
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

    case 'add-child':
      await createChildOfPage(page as unknown as NavPage)
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

    case 'copy-md':
      try {
        const blob = await documentationService.exportPageMarkdown(page.id)
        if (!blob) return
        const md = await blob.text()
        await copy(md)
      } catch (error) {
        console.error('Failed to copy markdown:', error)
      }
      break

    case 'copy-text':
      try {
        const blob = await documentationService.exportPageMarkdown(page.id)
        if (!blob) return
        const md = await blob.text()
        await copy(stripMarkdown(md))
      } catch (error) {
        console.error('Failed to copy text:', error)
      }
      break

    case 'subscribe':
      try {
        await documentationService.subscribeToPage(Number(page.id))
      } catch (error) {
        console.error('Failed to subscribe:', error)
      }
      break

    case 'history':
    case 'insights':
      // Navigate to the page (no-op if already there) AND signal
      // the page view to open the requested panel via shared
      // module state. URL stays clean — panel state isn't part
      // of the page identity, and using a query param made
      // re-clicks of an already-open panel a router no-op.
      if (route.path !== pageUrl) router.push(pageUrl)
      docPanel.open(actionId)
      break

    case 'print':
      // Print is a one-shot side effect, not panel state, so the
      // query-param signal is fine here. The page view consumes
      // and strips it.
      router.push({ path: pageUrl, query: { print: '1' } })
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

/** Cheap markdown-to-text strip for the "Copy as plain text"
 * action. Drops fenced code blocks, link/image syntax, headings,
 * emphasis markers, list bullets, and frontmatter. Not a full
 * commonmark parser — good enough for clipboard use where the
 * user just wants the words. */
function stripMarkdown(md: string): string {
  return md
    .replace(/^---[\s\S]*?---\n/, '') // YAML frontmatter
    .replace(/```[\s\S]*?```/g, '') // fenced code
    .replace(/`([^`]+)`/g, '$1') // inline code
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1') // images
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1') // links
    .replace(/^#{1,6}\s+/gm, '') // headings
    .replace(/^\s*[-*+]\s+/gm, '') // bullets
    .replace(/^\s*\d+\.\s+/gm, '') // numbered lists
    .replace(/^\s*>\s?/gm, '') // blockquotes
    .replace(/(\*\*|__)(.*?)\1/g, '$2') // bold
    .replace(/(\*|_)(.*?)\1/g, '$2') // italic
    .replace(/^---+$/gm, '') // hr
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

// Per-collection sort mode, persisted in localStorage so a
// reload restores the user's preference. Manual = honour the
// drag-set display_order; alpha = title ascending; recent =
// updated_at descending.
type SortMode = 'manual' | 'alpha' | 'recent'
// Localised sort labels. Reactive so a locale switch refreshes
// the menu without remount.
const SORT_LABELS = computed<Record<SortMode, string>>(() => ({
  manual: t('docs-nav-sort-manual'),
  alpha: t('docs-nav-sort-alpha'),
  recent: t('docs-nav-sort-recent'),
}))
const SORT_ORDER: SortMode[] = ['manual', 'alpha', 'recent']
const SORT_STORAGE_KEY = 'docs.collectionSort.v1'

function loadSortPrefs(): Record<number, SortMode> {
  try {
    const raw = localStorage.getItem(SORT_STORAGE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as Record<string, SortMode>
    const out: Record<number, SortMode> = {}
    for (const [k, v] of Object.entries(parsed)) {
      const id = Number(k)
      if (!Number.isNaN(id) && SORT_ORDER.includes(v)) out[id] = v
    }
    return out
  } catch {
    return {}
  }
}

const collectionSort = reactive<Record<number, SortMode>>(loadSortPrefs())

function persistSortPrefs() {
  try {
    localStorage.setItem(SORT_STORAGE_KEY, JSON.stringify(collectionSort))
  } catch {
    // Storage quota / private mode — silent best-effort.
  }
}

function currentSortMode(collectionId: number | undefined): SortMode {
  if (collectionId === undefined) return 'manual'
  return collectionSort[collectionId] ?? 'manual'
}

function setSortMode(collectionId: number, mode: SortMode) {
  if (mode === 'manual') {
    // Manual is the default; remove the override entirely so a
    // fresh collection without a saved preference reads as
    // "manual" without polluting localStorage with explicit
    // entries.
    delete collectionSort[collectionId]
  } else {
    collectionSort[collectionId] = mode
  }
  persistSortPrefs()
}

// Inline page creation. Creates a draft page in the target
// collection and routes the user to it so the title input is
// focused for immediate edit. Parent inheritance: when called
// from a page row, the new page becomes a child; when called
// from a collection header, the new page lands at the
// collection's root.
const creatingInCollection = ref<number | null>(null)
const creatingUnderPage = ref<number | string | null>(null)

async function createInCollection(collectionId: number) {
  if (creatingInCollection.value !== null) return
  creatingInCollection.value = collectionId
  try {
    const created = await createArticle({
      title: t('docs-nav-untitled'),
      icon: '📄',
      status: 'draft',
      parent_id: null,
      ...({ collection_id: collectionId } as Partial<Page>),
    })
    if (created) {
      collectionExpanded[collectionId] = true
      await loadCollectionPages(collectionId)
      router.push(docUrl({ slug: created.slug ?? '', id: created.id as number }))
    }
  } finally {
    creatingInCollection.value = null
  }
}

async function createChildOfPage(parentPage: NavPage) {
  const parentId = Number(parentPage.id)
  if (Number.isNaN(parentId) || creatingUnderPage.value !== null) return
  creatingUnderPage.value = parentPage.id
  try {
    const created = await createArticle({
      title: t('docs-nav-untitled'),
      icon: '📄',
      status: 'draft',
      parent_id: parentId,
    })
    if (created) {
      // Reload the owning collection so the new page appears in
      // the tree immediately. The backend's create-cascade has
      // already inserted the junction row for us.
      const collectionId = pageToCollectionMap.value[String(parentPage.id)]
      if (collectionId) {
        collectionExpanded[collectionId] = true
        await loadCollectionPages(collectionId)
      }
      router.push(docUrl({ slug: created.slug ?? '', id: created.id as number }))
    }
  } finally {
    creatingUnderPage.value = null
  }
}

// Collections data — populated from the sync pool (see the watchEffect
// in "Collection loading" below), not a REST load.
const docs = useSyncDocsStore()
const collections = ref<CollectionWithDetails[]>([])

function visiblePagesIn(collectionId: number): Page[] {
  const all = collectionPages[collectionId] ?? []
  return sortPages(all, currentSortMode(collectionId))
}

function sortPages(nodes: Page[], mode: SortMode): Page[] {
  if (mode === 'manual') return nodes
  const sorted = [...nodes]
  if (mode === 'alpha') {
    sorted.sort((a, b) => (a.title ?? '').localeCompare(b.title ?? ''))
  } else if (mode === 'recent') {
    sorted.sort((a, b) => {
      const aDate = (a as Page & { updated_at?: string }).updated_at ?? ''
      const bDate = (b as Page & { updated_at?: string }).updated_at ?? ''
      return bDate.localeCompare(aDate)
    })
  }
  // Recurse into children with the same mode so subtrees stay
  // consistent with their parent's order.
  return sorted.map((n) => ({
    ...n,
    children: sortPages(((n as Page & { children?: Page[] }).children) ?? [], mode),
  }))
}

// Per-collection expanded state (stored in localStorage)
const collectionExpanded = reactive<Record<number, boolean>>({})
// Per-collection loaded page trees (lazy loaded)
const collectionPages = reactive<Record<number, Page[]>>({})
// Track which collections have been loaded
const collectionLoaded = reactive<Record<number, boolean>>({})
// Map from page ID -> collection ID (for auto-expand on navigate)
const pageToCollectionMap = ref<Record<string, number>>({})
// Parent map for page hierarchy within collections
const pageParentMap = ref<Record<string, string | null>>({})

// True once the workspace bootstrap that carries documentation
// collections + pages has settled (see onMounted). Until then an empty
// `collections` list is ambiguous ("still loading" vs "genuinely no
// docs"), so we hold off on the empty state. The pool is cache-first
// (IndexedDB), so on a warm refresh the rows are already here and this
// flips true without the user ever seeing a loading state.
const docsReady = ref(false)

// Show the skeleton only when a cold bootstrap is genuinely slow
// (>300ms with nothing cached yet). Warm refreshes hydrate from the
// pool instantly and never flash it; fast cold loads stay quiet and
// let the tree fill in. Mirrors AsyncBoundary's pendingDelay.
const showSkeleton = useDelayedFlag(
  () => !docsReady.value && collections.value.length === 0,
  300,
)

// ============================================================================
// Collection loading
// ============================================================================

/** Adapt a pool collection row to the richer CollectionWithDetails the
 *  sidebar + modals are typed against. The visibility fields and the
 *  Yjs description_doc_id are not in the sync projection; the nav never
 *  reads them (the edit modal edits text only, the visibility modal
 *  self-fetches), so empty placeholders are safe. page_count derives
 *  from the pool. */
function toCollectionWithDetails(c: DocCollectionRow, pageCount: number): CollectionWithDetails {
  return {
    id: c.id,
    uuid: c.uuid,
    name: c.name,
    slug: c.slug,
    description: c.description,
    description_text: c.description_text,
    description_doc_id: '',
    hide_titles_from_non_members: c.hide_titles_from_non_members,
    require_verification: c.require_verification,
    icon: c.icon,
    color: c.color,
    is_system: c.is_system,
    created_by: c.created_by,
    created_at: c.created_at,
    updated_at: c.updated_at,
    visible_to_groups: [],
    visible_to_users: [],
    is_public: false,
    page_count: pageCount,
  }
}

const buildParentMapInto = (
  tree: Page[],
  parentId: string | null,
  target: Record<string, string | null>,
) => {
  for (const page of tree) {
    target[String(page.id)] = parentId
    if (page.children && page.children.length > 0) {
      buildParentMapInto(page.children, String(page.id), target)
    }
  }
}

// Collections whose persisted expand state has already been restored.
// Non-reactive so reading/writing it inside the watchEffect doesn't make
// it a dependency (which would rebuild every tree on each expand toggle).
const expandRestored = new Set<number>()

// Single source of truth: rebuild the collection list, per-collection
// active page trees, and the lookup maps from the sync pool whenever it
// changes (bootstrap, SSE, delta). Replaces the REST loads and the
// discrete documentation-updated / collection-updated listeners — live
// edits flow in through the pool.
watchEffect(() => {
  const cols = docs.collectionsSorted
  const pages = docs.allPages

  const newPageToCollection: Record<string, number> = {}
  const newParent: Record<string, string | null> = {}
  const allTreePages: Page[] = []

  collections.value = cols.map((c) => {
    // Restore persisted expand state the first time a collection appears
    // (collections stream in from the pool, possibly after mount).
    if (!expandRestored.has(c.id)) {
      expandRestored.add(c.id)
      const stored = localStorage.getItem(`docNavCollectionExpanded_${c.id}`)
      if (stored != null) collectionExpanded[c.id] = stored === 'true'
    }
    const rows = pages.filter((p) => p.collection_id === c.id)
    const activeRows = rows.filter(isActivePage)
    const tree = buildPageTree(activeRows).map(nodeToPage)
    collectionPages[c.id] = tree
    collectionLoaded[c.id] = true
    for (const r of rows) newPageToCollection[String(r.id)] = c.id
    buildParentMapInto(tree, null, newParent)
    allTreePages.push(...tree)
    return toCollectionWithDetails(c, activeRows.length)
  })

  pageToCollectionMap.value = newPageToCollection
  pageParentMap.value = newParent
  docNavStore.setPages(allTreePages)
})


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

// Collection page trees are derived from the sync pool by the
// watchEffect above; there is nothing to fetch. Kept as an async no-op
// so existing call sites (lazy-expand, auto-expand, drag commit) need
// no changes — the pool already holds every page.
const loadCollectionPages = async (_collectionId: number) => {}

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

const handlePageDragStart = (id: string | number, _event: DragEvent) => {
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

const wouldCreateCircularReference = (draggedId: string | number, targetId: string | number, _position: 'above' | 'inside' | 'below'): boolean => {
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
    // Collections + pages come from the sync pool (watchEffect above);
    // only the starred list needs an explicit fetch.
    getStarredPages().then(pages => docNavStore.setStarredPages(pages));

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
  }

  docNavStore.updateSidebarForScreenSize()
  window.addEventListener('resize', handleResize)

  // Mark docs ready once the workspace bootstrap (which streams the
  // documentation collections + pages into the pool) settles. Idempotent
  // and cache-first: resolves immediately on a warm refresh, so the empty
  // state only ever appears when there genuinely are no collections.
  // `.finally` so a bootstrap error still releases the loading gate
  // rather than leaving the nav stuck blank.
  subscribe('workspace:1').finally(() => { docsReady.value = true });
})

onUnmounted(() => {
  window.removeEventListener('resize', handleResize)
})

// Collections and pages re-derive from the pool automatically; only the
// starred list (not a pool aggregate) needs an explicit refresh.
const reloadSidebar = async () => {
  getStarredPages().then(pages => docNavStore.setStarredPages(pages));
};

defineExpose({ reloadSidebar });
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

    <!-- Loading skeleton — only on a genuinely slow cold load (>300ms
         with nothing cached). Warm refreshes hydrate from the pool
         instantly and never flash it. -->
    <div v-if="showSkeleton" class="py-1 px-2">
      <div v-for="i in 3" :key="i" class="flex items-center gap-1.5 py-1">
        <div class="flex-shrink-0" style="width: 8px"></div>
        <div class="w-4 h-4 rounded bg-surface-hover/50 animate-pulse flex-shrink-0"></div>
        <div class="flex-1 h-3 rounded bg-surface-hover/60 animate-pulse" :style="{ maxWidth: `${50 + (i % 3) * 15}%` }"></div>
      </div>
    </div>

    <!-- Starred Pages Section -->
    <div v-if="!showSkeleton && starredPages.length > 0" class="py-1">
      <!-- Starred Header -->
      <div
        class="group relative flex items-center py-1 pr-2 rounded text-xs cursor-pointer transition-all duration-150 text-secondary hover:text-primary hover:bg-surface-hover"
        @click="docNavStore.toggleStarredExpanded()"
      >
        <span class="flex-shrink-0" style="width: 8px"></span>
        <span class="flex-shrink-0 w-5 flex items-center justify-center">
          <Icon
            name="chevronRight"
            size="xs"
            class="text-tertiary transition-transform duration-200"
            :class="{ 'rotate-90': isStarredExpanded }"
          />
        </span>
        <span class="flex-shrink-0 ml-0.5 text-amber-500 inline-flex">
          <Icon name="star" />
        </span>
        <span class="flex-1 truncate min-w-0 ml-1 font-medium">{{ $t('docs-nav-starred') }}</span>
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
    <div v-if="!showSkeleton" class="py-1">
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
          @pointerdown="onCollectionPointerdown(collection, $event)"
          @pointerup="collectionLongPress.listeners.pointerup"
          @pointermove="collectionLongPress.listeners.pointermove"
          @pointercancel="collectionLongPress.listeners.pointercancel"
          @pointerleave="collectionLongPress.listeners.pointerleave"
        >
          <!-- Indent spacing -->
          <span class="flex-shrink-0" style="width: 8px"></span>

          <!-- Icon / Expand Toggle. The chevron only appears when
               the collection has pages, since an empty collection
               with a dropdown affordance falsely implies content
               sits behind it. The icon stays as a static glyph in
               that case. -->
          <span
            class="flex-shrink-0 w-5 h-5 flex items-center justify-center"
            @click.stop="(collection.page_count ?? 0) > 0 ? toggleCollectionExpanded(collection.id) : null"
          >
            <template v-if="(collection.page_count ?? 0) > 0">
              <Icon
                name="chevronRight"
                size="xs"
                class="hidden group-hover:block text-tertiary transition-transform duration-200"
                :class="{ 'rotate-90': collectionExpanded[collection.id] }"
              />
              <CollectionIcon
                :icon="collection.icon"
                :color="collection.color"
                size="xs"
                class="group-hover:hidden"
              />
            </template>
            <CollectionIcon
              v-else
              :icon="collection.icon"
              :color="collection.color"
              size="xs"
            />
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

          <NavRowActions
            v-if="renamingCollectionId !== collection.id"
            :label="collection.name"
            :creating="creatingInCollection === collection.id"
            @more="(e) => openCollectionMenu(collection, e)"
            @add="createInCollection(collection.id)"
          />
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
              <!-- Pages tree (filtered by sidebar search). Pages stream
                   in from the pool, so there's no per-collection fetch to
                   show a loading state for. -->
              <ul v-if="visiblePagesIn(collection.id).length > 0" class="flex flex-col">
                <DocumentationNavItem
                  v-for="page in visiblePagesIn(collection.id)"
                  :key="page.id"
                  :page="page"
                  :level="1"
                  :dragged-page-id="draggedPageId"
                  :creating-child-of-id="creatingUnderPage"
                  :is-dragging="String(draggedPageId) === String(page.id)"
                  :is-drop-target="String(dropTargetId) === String(page.id) && dropPosition === 'inside'"
                  @toggle-expand="handleToggleExpand"
                  @page-click="handlePageClick"
                  @drag-start="handlePageDragStart"
                  @drag-end="handlePageDragEnd"
                  @drag-over="(id, event, position, level) => handlePageDragOver(id, event, position, level)"
                  @drop="handlePageDrop"
                  @context-menu="handleNavContextMenu"
                  @add-child="(p) => createChildOfPage(p as unknown as NavPage)"
                />
              </ul>
            </template>
          </div>
        </div>
      </div>

      <!-- Empty State — only once the bootstrap has settled, so a
           cold load doesn't flash "no documentation" before the pool
           populates. -->
      <div v-if="docsReady && collections.length === 0" class="px-4 py-8 text-center">
        <div class="text-tertiary text-sm">{{ $t('docs-nav-empty') }}</div>
      </div>
    </div>

    <!-- Context Menu. Always mounted, toggled via :open so the
         enter/leave fade-scale transition Popover provides
         actually plays. v-if would unmount the whole subtree
         before the leave transition could run. -->
    <ContextMenu
      :open="showContextMenu"
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
    :title="pendingDeleteCollection ? $t('docs-nav-confirm-delete-collection-title', { name: pendingDeleteCollection.name }) : $t('docs-nav-confirm-delete-collection-fallback')"
    :message="$t('docs-nav-confirm-delete-collection-message')"
    :confirm-label="$t('docs-nav-confirm-delete')"
    @confirm="doDeleteCollection"
    @close="pendingDeleteCollection = null"
  />

  <CollectionModal
    mode="edit"
    :show="editingCollection !== null"
    :collection="editingCollection"
    @close="editingCollection = null"
    @saved="handleCollectionEdited"
  />
</template>

<style scoped>
.documentation-nav {
  position: relative;
  /* Lets the parent layout decide the sidebar height; the
     internal column then scrolls when the collection list is
     taller than the viewport. Without this, long page lists
     push off the bottom of short viewports (laptop with browser
     chrome, tablet landscape) with no scroll affordance. */
  display: flex;
  flex-direction: column;
  min-height: 0;
  max-height: 100%;
  overflow-y: auto;
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
