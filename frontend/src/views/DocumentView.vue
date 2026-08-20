<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { shareableRouteUrl } from '@/utils/shareUrl'
import { useFluent } from 'fluent-vue'
import { formatDate } from '@nosdesk/core/utils/dateUtils'
import { docUrl, slugify } from '@nosdesk/core/utils/docUrl'
import { useTitleManager } from '@/composables/useTitleManager'
import { useDocumentation } from '@/composables/useDocumentation'
import { useSyncDocsStore, type DocPageRow } from '@nosdesk/core/sync/stores/documentation'
import { useClipboard } from '@/composables/useClipboard'
import { useDocumentPanelState } from '@/composables/useDocumentPanelState'
import { useMyWorkspacesStore } from '@/stores/myWorkspaces'
import { buildCollabDocId } from '@nosdesk/core/utils/collabDocId'
import documentationService from '@nosdesk/core/services/documentationService'
import type { Page, Article } from '@nosdesk/core/services/documentationService'
import CollaborativeEditor from '@/components/CollaborativeEditor.vue'
import BackButton from '@/components/common/BackButton.vue'
import Icon from '@/components/common/Icon.vue'
import Spinner from '@/components/common/Spinner.vue'
import DocumentActionsMenu from '@/components/documentationComponents/DocumentActionsMenu.vue'
import MoveDocumentModal from '@/components/documentationComponents/MoveDocumentModal.vue'
import DocumentationBreadcrumb from '@/components/documentationComponents/DocumentationBreadcrumb.vue'
import CollectionManager from '@/components/documentationComponents/CollectionManager.vue'
import PullToRefresh from '@/components/common/PullToRefresh.vue'
import PagePermissionsModal from '@/components/documentationComponents/PagePermissionsModal.vue'
import { docsEmitter } from '@nosdesk/core/services/docsEmitter'
import RevisionHistory from '@/components/editor/RevisionHistory.vue'
import DocumentInsightsPanel from '@/components/documentationComponents/DocumentInsightsPanel.vue'
import DocumentAuthorBadge from '@/components/documentationComponents/DocumentAuthorBadge.vue'
import PageTicketLinksPanel from '@/components/documentationComponents/PageTicketLinksPanel.vue'
import apiClient from '@nosdesk/core/apiClient'
import { useAuthStore } from '@/stores/auth'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import { useCollabSessionStore, type ConnectionStatus } from '@/stores/collabSession'

const route = useRoute()
const router = useRouter()
const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const titleManager = useTitleManager()

// Pull-to-refresh (Tauri app) binds to the outer scroll container. The
// ProseMirror editor is contenteditable, so the composable's ignore
// list keeps a pull from starting on the editing surface — the gesture
// only fires from the surrounding chrome at scroll-top.
const scrollEl = ref<HTMLElement | null>(null)
const authStore = useAuthStore()
const docNavStore = useDocumentationNavStore()
const { copied: copiedLink, copy: copyToClipboard } = useClipboard()

// Use shared documentation composable
const {
  deletePage,
  documentationNavStore,
} = useDocumentation()
const docs = useSyncDocsStore()

// Live-collaboration connection for this doc. The real-time mechanism on
// a document is the Yjs WebSocket the editor opens (owned, per-doc, by
// the collab session store), not SSE — SSE presence is being retired for
// the WS ephemeral plane. When
// per-doc viewer presence lands there, this indicator grows into it.
const collabStore = useCollabSessionStore()
const liveStatus = computed<ConnectionStatus>(() =>
  docId.value ? collabStore.connectionStatus[docId.value] ?? 'connecting' : 'disconnected',
)

// Document state — use preloaded data from route guard when available
const preloaded = route.meta.preloadedDocument as Page | undefined
const document = ref<Page | Article | null>(preloaded ?? null)
const isLoading = ref(!preloaded)
// Two-way bound to DocumentAuthorBadge so the in-row "needs
// verification" chip can open the same popover from a different
// trigger location.
const verificationOpen = ref(false)
const isSaving = ref(false)
const saveMessage = ref('')
const showSuccessMessage = ref(false)

// Content editing — initialize from preloaded data if available
const editContent = ref(preloaded?.content || '')
const editTitle = ref(preloaded?.title || '')
const documentIcon = ref(preloaded?.icon || '📄')

// Ref for the title h1 element
const titleElementRef = ref<HTMLElement | null>(null)

// Debounced title save
let titleUpdateTimeout: ReturnType<typeof setTimeout> | null = null

// Revision-history and Insights panels are both derived from a
// single source of truth: the shared `useDocumentPanelState`
// composable. Computeds (not mirrored refs) so there's no
// double-state-and-watch ceremony — `activePanel` is the only
// thing anyone writes; everything else falls out reactively.
const docPanel = useDocumentPanelState()
const showRevisionHistory = computed(() => docPanel.activePanel.value === 'history')
const showInsights = computed(() => docPanel.activePanel.value === 'insights')
const editorRef = ref<InstanceType<typeof CollaborativeEditor> | null>(null)

// Plain-text snapshot for the Insights panel. Re-pulled when the
// panel opens so stats reflect the editor's current content;
// kept as a ref rather than a computed because we don't want to
// reactively re-run on every keystroke.
const insightsText = ref('')

function refreshInsightsText() {
  insightsText.value = editorRef.value?.getTextContent?.() ?? ''
}

watch(showInsights, (open) => {
  if (open) refreshInsightsText()
})

// Subscription state
const isSubscribed = ref(false)

// Starred state
const isStarred = ref(false)

// Computed helpers
const currentPageId = computed(() => document.value?.id ?? null)
const isDocumentPage = computed(() => !!document.value)

const handleCopyLink = () => {
  const slug = document.value?.slug || document.value?.id
  if (slug == null) return
  // Workspace-scoped in path mode so a shared link opens the right tenant.
  const url = shareableRouteUrl('documentation-page', { path: String(slug) })
  copyToClipboard(url)
}

// Emits
const emit = defineEmits<{
  (e: 'update:title', title: string): void
  (e: 'update:document', document: { id: string; title: string; icon: string; slug?: string } | null): void
}>()

// Document object for header
const documentObj = computed(() => {
  if (!document.value) return null
  return {
    id: String(document.value.id),
    title: document.value.title,
    icon: document.value.icon || documentIcon.value,
    slug: document.value.slug
  }
})

// Doc ID for CollaborativeEditor. Pages own their Yjs doc, keyed by the
// page's immutable UUID (see utils/collabDocId.ts) so stale IDB caches
// across a database reset can't repopulate the new doc.
//
// Returns `null` (rather than a fabricated default) until the
// workspace UUID resolves. The editor render is gated below so the
// user never sees a half-formed doc; documentation-new acts as
// the local-only fallback for the brand-new-page wizard, where
// there's nothing in Yjs to share yet.
const workspaces = useMyWorkspacesStore()

// Integer page id for the revision + embedding endpoints, which are
// integer-keyed while the collab docId carries the uuid. Null until the page
// loads, and for the brand-new-page wizard.
const pageId = computed(() => {
  const raw = document.value && 'id' in document.value ? document.value.id : null
  const n = Number(raw)
  return Number.isFinite(n) && n > 0 ? n : undefined
})

const docId = computed(() => {
  const uuid = workspaces.activeWorkspace?.workspace_uuid
  if (!uuid) return null
  if (document.value && 'uuid' in document.value && document.value.uuid) {
    return buildCollabDocId(uuid, 'doc', document.value.uuid)
  }
  // Brand-new page wizard: no server doc yet, no IDB collision
  // possible. The literal sentinel is fine because the editor
  // doesn't try to sync this id with the server.
  return 'documentation-new'
})

// Navigation helpers
const fallbackRoute = computed(() => '/documentation')

const backButtonLabel = computed(() => t('doc-detail-back-to-documentation'))

// Content update handler
const updateContent = (newContent: string) => {
  editContent.value = newContent
  if (document.value) {
    document.value.content = newContent
  }
}

// Title update handler with debounced save
const updateTitle = (newTitle: string) => {
  editTitle.value = newTitle

  if (document.value) {
    emit('update:title', newTitle)
    titleManager.setCustomTitle(newTitle)

    const slug = slugify(newTitle)
    document.value.title = newTitle
    document.value.slug = slug

    // Debounce backend save
    if (titleUpdateTimeout) {
      clearTimeout(titleUpdateTimeout)
    }
    titleUpdateTimeout = setTimeout(() => {
      saveTitleChanges()
    }, 500)
  }
}

// Save title changes to backend
const saveTitleChanges = async () => {
  if (!currentPageId.value) return

  const newSlug = slugify(editTitle.value)

  try {
    await documentationService.updatePage(currentPageId.value, { title: editTitle.value, slug: newSlug })
    documentationNavStore.updatePageField(currentPageId.value, 'title', editTitle.value)
    documentationNavStore.updatePageField(currentPageId.value, 'slug', newSlug)
  } catch (error) {
    console.error('Failed to save title:', error)
  }
}

// Toolbar toggles. Both go through the shared panel store so
// the sidebar context-menu items and the in-page toolbar stay
// in sync — closing here updates activePanel, so a subsequent
// click from either surface transitions cleanly.
const toggleRevisionHistory = () => {
  showRevisionHistory.value ? docPanel.close() : docPanel.open('history')
}

// The editor owns which revision is on screen, so the panel highlight follows
// it whether the exit came from the list or from the overlay's own button.
const activeRevisionNumber = computed(() => editorRef.value?.currentRevisionNumber ?? null)

const handleSelectRevision = async (revisionNumber: number | null) => {
  if (!editorRef.value) return

  if (revisionNumber === null) {
    editorRef.value.exitRevisionView()
    return
  }

  try {
    if (!currentPageId.value) return

    const response = await apiClient.get(
      `/collaboration/docs/${currentPageId.value}/revisions/${revisionNumber}`
    )
    editorRef.value.viewSnapshot(response.data)
  } catch (error) {
    console.error('Failed to fetch revision:', error)
  }
}

const handleCloseRevisionHistory = () => {
  // Clear the shared state so the next "Revision history" click
  // from the sidebar transitions activePanel from null → 'history'
  // (which fires the watcher) instead of going same → same.
  docPanel.close()
  if (editorRef.value?.isViewingRevision) {
    editorRef.value.exitRevisionView()
  }
}

const handleCloseInsights = () => {
  docPanel.close()
}

const handleRevisionRestored = () => {
  fetchContent()
}

// Delete handler
const handleDeletePage = async () => {
  if (!document.value) return

  try {
    isSaving.value = true
    saveMessage.value = t('doc-detail-toast-deleting')
    showSuccessMessage.value = true

    const success = await deletePage(document.value.id)

    if (success) {
      saveMessage.value = t('doc-detail-toast-deleted')
      setTimeout(() => {
        router.push('/documentation')
      }, 1000)
    } else {
      saveMessage.value = t('doc-detail-toast-delete-error')
      setTimeout(() => {
        showSuccessMessage.value = false
      }, 3000)
    }
  } catch (error) {
    console.error('Error deleting page:', error)
    saveMessage.value = t('doc-detail-toast-delete-error')
    setTimeout(() => {
      showSuccessMessage.value = false
    }, 3000)
  } finally {
    isSaving.value = false
  }
}

// Move modal state
const showMoveModal = ref(false)

// Collection state
const showCollectionManager = ref(false)

// Permissions modal state
const showPermissionsModal = ref(false)

const exportAsMarkdown = async () => {
  if (!currentPageId.value) return
  try {
    const blob = await documentationService.exportPageMarkdown(currentPageId.value)
    if (!blob) return
    const url = URL.createObjectURL(blob)
    const a = window.document.createElement('a')
    a.href = url
    const filename = (document.value?.slug || document.value?.title?.toLowerCase().replace(/\s+/g, '-') || 'document') + '.md'
    a.download = filename
    window.document.body.appendChild(a)
    a.click()
    window.document.body.removeChild(a)
    URL.revokeObjectURL(url)
  } catch (err) {
    console.error('Failed to export markdown:', err)
  }
}

// Consolidated status update handler
async function updatePageStatus(status: string, opts?: { redirect?: string; apiSuffix?: string }) {
  if (!currentPageId.value) return
  try {
    if (opts?.apiSuffix) {
      await documentationService.restorePage(currentPageId.value)
    } else {
      await documentationService.updatePage(currentPageId.value, { status })
    }
    if (document.value) document.value.status = status
    documentationNavStore.updatePageField(currentPageId.value, 'status', status)
    if (opts?.redirect) {
      router.push(opts.redirect)
    } else {
      documentationNavStore.refreshPages()
    }
  } catch (error) {
    console.error(`Failed to update page status to ${status}:`, error)
  }
}

const handleArchivePage = () => updatePageStatus('archived', { redirect: '/documentation' })
const handleRestorePage = () => updatePageStatus('draft', { apiSuffix: '/restore' })
const handlePublishPage = () => updatePageStatus('published')
const handleUnpublishPage = () => updatePageStatus('draft')

const handlePageMoved = () => {
  showMoveModal.value = false
  documentationNavStore.refreshPages()
  fetchContent()
}

// Handle duplicate page
const handleDuplicatePage = async () => {
  if (!document.value) return

  try {
    const newPage = await documentationService.createArticle({
      title: t('doc-detail-duplicate-suffix', { title: document.value.title }),
      content: document.value.content || '',
      description: document.value.description || '',
      status: 'draft',
      icon: document.value.icon || '📄',
    })

    if (newPage?.id) {
      docsEmitter.emit('doc:created', { id: newPage.id })
      documentationNavStore.refreshPages()
      router.push(docUrl(newPage))
    }
  } catch (error) {
    console.error('Failed to duplicate page:', error)
  }
}

// Fetch document content
const fetchContent = async () => {
  // Skip fetch if preloaded data was already consumed on mount
  if (document.value && !isLoading.value && route.meta.preloadedDocument) {
    route.meta.preloadedDocument = undefined
    return
  }
  isLoading.value = true

  // Load document by path
  const path = route.params.path as string

  if (!path) {
    router.push('/documentation')
    return
  }

  try {
    const result = await documentationService.getPageByPath(path)

    if (result) {
      if ('children' in result && Array.isArray(result.children)) {
        document.value = result
        editContent.value = document.value.content || ''
        editTitle.value = document.value.title
        documentIcon.value = document.value.icon || 'mdi-folder-outline'
        emit('update:title', document.value.title)
      } else if ('id' in result) {
        const articleData = await documentationService.getArticleById(String(result.id))

        if (articleData) {
          document.value = articleData
          editContent.value = document.value.content || ''
          editTitle.value = document.value.title
          documentIcon.value = document.value.icon || 'mdi-text-box-outline'
          emit('update:title', document.value.title)
        } else {
          router.push('/documentation')
          return
        }
      }
    } else {
      router.push('/documentation')
      return
    }
  } catch (error) {
    console.error('Error fetching content:', error)
    router.push('/documentation')
    return
  } finally {
    isLoading.value = false

    // Fetch subscription and starred status for the loaded page
    if (currentPageId.value) {
      documentationService.getPageSubscription(Number(currentPageId.value)).then(subscribed => {
        isSubscribed.value = subscribed
      })
      documentationService.getPageStarred(Number(currentPageId.value)).then(starred => {
        isStarred.value = starred
      })
    }
  }
}

// Subscription handlers
const handleSubscribe = async () => {
  if (!currentPageId.value) return
  const success = await documentationService.subscribeToPage(Number(currentPageId.value))
  if (success) {
    isSubscribed.value = true
  }
}

const handleUnsubscribe = async () => {
  if (!currentPageId.value) return
  const success = await documentationService.unsubscribeFromPage(Number(currentPageId.value))
  if (success) {
    isSubscribed.value = false
  }
}

// Star handlers
const handleStar = async () => {
  if (!currentPageId.value || !document.value) return
  const success = await documentationService.starPage(Number(currentPageId.value))
  if (success) {
    isStarred.value = true
    docNavStore.addStarredPage({
      page_id: Number(document.value.id),
      title: document.value.title,
      slug: document.value.slug,
      icon: document.value.icon || null,
      starred_at: new Date().toISOString(),
    })
  }
}

const handleUnstar = async () => {
  if (!currentPageId.value) return
  const success = await documentationService.unstarPage(Number(currentPageId.value))
  if (success) {
    isStarred.value = false
    docNavStore.removeStarredPage(Number(currentPageId.value))
  }
}

// Live metadata sync from the sync pool. When another client edits the
// current page's title / slug / icon, the metadata_changed event lands
// in the pool and this watch mirrors it into local state. Replaces the
// discrete documentation-updated SSE listener. The body itself flows
// through the Yjs collaboration channel, not here. Guards on value
// changes so the user's own in-progress title edit isn't clobbered.
const poolPage = docs.pageById(() => {
  const id = document.value?.id
  return id == null ? null : Number(id)
})
watch(
  (): Pick<DocPageRow, 'title' | 'slug' | 'icon'> | null => {
    const p = poolPage.value
    return p ? { title: p.title, slug: p.slug, icon: p.icon } : null
  },
  (meta) => {
    if (!meta) return
    if (document.value) {
      document.value.title = meta.title
      document.value.slug = meta.slug
      if (meta.icon != null) document.value.icon = meta.icon
    }
    if (editTitle.value !== meta.title) {
      editTitle.value = meta.title
      titleManager.setCustomTitle(meta.title)
      emit('update:title', meta.title)
      if (titleElementRef.value && titleElementRef.value.textContent !== meta.title) {
        titleElementRef.value.textContent = meta.title
      }
    }
    if (meta.icon != null && documentIcon.value !== meta.icon) {
      documentIcon.value = meta.icon
    }
  },
)

// Lifecycle
// Print is a one-shot side effect, not panel state, so it stays
// as a query param. Consume + strip so a refresh doesn't re-fire.
function consumePrintQuery() {
  if (route.query.print === '1') {
    setTimeout(() => window.print(), 50)
    router.replace({ path: route.path, query: { ...route.query, print: undefined } })
  }
}

watch(() => route.query.print, consumePrintQuery)

onMounted(() => {
  fetchContent()
  consumePrintQuery()

  // Register save handlers for SiteHeader title/icon edits
  titleManager.onDocumentTitleSave(async (title: string) => {
    if (!currentPageId.value) return
    const newSlug = slugify(title)
    await documentationService.updatePage(currentPageId.value, { title, slug: newSlug })
    documentationNavStore.updatePageField(currentPageId.value, 'title', title)
    documentationNavStore.updatePageField(currentPageId.value, 'slug', newSlug)
    if (document.value) document.value.slug = newSlug
  })

  titleManager.onDocumentIconSave(async (icon: string) => {
    if (!currentPageId.value) return
    await documentationService.updatePage(currentPageId.value, { icon })
    documentationNavStore.updatePageField(currentPageId.value, 'icon', icon)
  })
})

onUnmounted(() => {
  titleManager.onDocumentTitleSave(null)
  titleManager.onDocumentIconSave(null)
  if (titleUpdateTimeout) {
    clearTimeout(titleUpdateTimeout)
  }
})

// Watch for route changes
watch(() => route.params.path, () => {
  fetchContent()
})

// Emit document object when it changes
watch(documentObj, (newDocument) => {
  if (newDocument) {
    emit('update:document', newDocument)
  }
}, { immediate: true })
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <PullToRefresh :target="scrollEl" />
    <!-- Header bar -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <!--
        Row sizes itself to its children; every toolbar button in this
        view (BackButton, publish, star, copy, menu) is `h-8`, so the
        row is a stable 3 rem whether or not the right-side cluster is
        mounted. `flex-nowrap` keeps everything on one line on narrow
        viewports instead of wrapping to a second row.
      -->
      <div class="p-2 flex items-center gap-2 flex-nowrap">
        <!-- Back button -->
        <BackButton :fallbackRoute="fallbackRoute" :label="backButtonLabel" />

        <!-- Spacer -->
        <div class="flex-1"></div>

        <!-- Saving indicator -->
        <span v-if="isSaving" class="text-accent flex items-center gap-1 text-xs">
          <Spinner size="xs" />
          {{ $t('doc-detail-saving') }}
        </span>

        <!-- Publish button for unpublished pages -->
        <button
          v-if="document && document.status !== 'published'"
          @click="handlePublishPage"
          class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md bg-status-success text-white hover:opacity-90 transition-colors"
        >
          <Icon name="check" />
          <span class="hidden sm:inline">{{ $t('doc-detail-publish') }}</span>
        </button>

        <!-- Star button -->
        <button
          v-if="isDocumentPage"
          @click="isStarred ? handleUnstar() : handleStar()"
          class="p-1.5 rounded-md hover:bg-surface-hover transition-colors"
          :class="isStarred ? 'text-brand-gold' : 'text-secondary hover:text-primary'"
          :title="isStarred ? $t('doc-detail-unstar') : $t('doc-detail-star')"
        >
          <svg class="w-5 h-5" :fill="isStarred ? 'currentColor' : 'none'" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z" />
          </svg>
        </button>

        <!-- Copy link button -->
        <button
          v-if="isDocumentPage"
          @click="handleCopyLink"
          class="p-1.5 rounded-md hover:bg-surface-hover transition-colors text-secondary hover:text-primary"
          :title="copiedLink ? $t('doc-detail-copied') : $t('doc-detail-copy-link')"
        >
          <Icon v-if="!copiedLink" name="link" size="md" />
          <Icon v-else name="check" size="md" class="text-status-success" />
        </button>

        <!-- Document actions menu -->
        <DocumentActionsMenu
          v-if="isDocumentPage"
          :page-id="document?.id || ''"
          :page-title="editTitle || document?.title || ''"
          :page-slug="document?.slug || ''"
          :page-status="document?.status || 'draft'"
          @delete="handleDeletePage"
          @duplicate="handleDuplicatePage"
          @archive="handleArchivePage"
          @restore="handleRestorePage"
          @publish="handlePublishPage"
          @unpublish="handleUnpublishPage"
          @move="showMoveModal = true"
          @export="exportAsMarkdown"
          @collections="showCollectionManager = true"
          :show-permissions="authStore.isAdmin"
          :is-subscribed="isSubscribed"
          @permissions="showPermissionsModal = true"
          @subscribe="handleSubscribe"
          @unsubscribe="handleUnsubscribe"
          @insights="docPanel.open('insights')"
          @history="docPanel.open('history')"
        />
      </div>
    </div>

    <!-- Main content -->
    <div ref="scrollEl" class="flex flex-col flex-1 overflow-auto bg-gradient-to-b from-bg-app to-bg-surface items-center">
      <!--
        Loading fallback. Document pages are preloaded by the route
        guard and typical fetches land well under Nielsen's 1 s flow
        boundary, so any indicator flashes and hurts more than it
        helps. Render nothing — the header + nav stay visible and the
        body fills in when data arrives.
      -->
      <div v-if="isLoading" />

      <!-- Document Content View -->
      <div v-else-if="document" class="w-full flex">
        <!-- Main Content Area -->
        <div class="flex-1 flex justify-center">
          <div class="w-full max-w-3xl px-4 sm:px-6 lg:px-8 py-6 sm:py-8 flex flex-col">
            <!-- Breadcrumb -->
            <DocumentationBreadcrumb
              :page-id="document.id || ''"
              :parent-id="document.parent_id || null"
              class="mb-4"
            />

            <!-- Document Header -->
            <div class="mb-6">
              <!-- Title -->
              <div class="mb-4">
                <h1
                  ref="titleElementRef"
                  contenteditable="true"
                  @blur="updateTitle(($event.target as HTMLElement).textContent || '')"
                  @keydown.enter.prevent="($event.target as HTMLElement).blur()"
                  class="text-2xl sm:text-3xl font-bold text-primary break-words leading-tight tracking-tight outline-none focus:ring-1 focus:ring-accent/30 rounded px-1 -mx-1"
                >
                  {{ editTitle || document.title || $t('doc-detail-untitled') }}
                </h1>
              </div>

              <!-- Metadata bar -->
              <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3 pt-4 pb-2 border-t border-subtle">
                <!-- Metadata -->
                <div class="flex flex-wrap items-center gap-x-3 gap-y-2 text-xs text-tertiary">
                  <!-- Status Badge -->
                  <span v-if="document.status === 'draft'" class="text-[10px] px-1.5 py-0.5 rounded bg-status-warning-muted text-status-warning font-medium">{{ $t('doc-detail-status-draft') }}</span>
                  <span v-else-if="document.status === 'archived'" class="text-[10px] px-1.5 py-0.5 rounded bg-surface-alt text-secondary font-medium">{{ $t('doc-detail-status-archived') }}</span>

                  <!--
                    Verification status chip — sits next to the
                    Draft/Archived badges so trust state reads as a
                    property of the document rather than a property
                    of the author. The never-verified prompt only
                    shows when the page's collection opts into
                    required verification; otherwise an unverified
                    page is neutral (no chip). Stale always shows.
                    The fresh case is conveyed by the inline check on
                    the author badge.
                  -->
                  <button
                    v-if="document.created_by && document.requires_verification && !document.verified_at"
                    type="button"
                    class="text-[10px] px-1.5 py-0.5 rounded font-medium bg-accent/10 text-accent hover:bg-accent/20 transition-colors flex items-center gap-1"
                    :title="$t('doc-detail-needs-verification-title')"
                    @click="verificationOpen = true"
                  >
                    <span class="w-1.5 h-1.5 rounded-full bg-accent" aria-hidden="true" />
                    {{ $t('doc-detail-needs-verification') }}
                  </button>
                  <button
                    v-else-if="document.is_stale"
                    type="button"
                    class="text-[10px] px-1.5 py-0.5 rounded font-medium bg-status-warning-muted text-status-warning hover:bg-status-warning/25 transition-colors flex items-center gap-1 animate-pulse"
                    :title="$t('doc-detail-verification-stale-title')"
                    @click="verificationOpen = true"
                  >
                    <Icon name="warning" size="xs" />
                    {{ $t('doc-detail-verification-stale') }}
                  </button>

                  <!-- Author + verification (consolidated). Inline
                       glyph confirms the *fresh* state; click opens
                       a popover with full metadata and cadence
                       picker. Two-way bind on `open` lets the
                       sibling status chip open the same popover. -->
                  <DocumentAuthorBadge
                    v-if="document.created_by"
                    v-model:open="verificationOpen"
                    :page="document"
                    :can-verify="authStore.isTechnician || authStore.isAdmin"
                    @changed="fetchContent"
                  />

                  <!-- Live-collaboration connection (Yjs WebSocket) -->
                  <div
                    class="w-2 h-2 rounded-full flex-shrink-0"
                    :class="{
                      'bg-status-success animate-pulse': liveStatus === 'connected',
                      'bg-status-warning animate-pulse': liveStatus === 'connecting',
                      'bg-status-error': liveStatus === 'disconnected',
                    }"
                    :title="liveStatus === 'connected' ? $t('doc-detail-live-active') : liveStatus === 'connecting' ? $t('doc-detail-live-connecting') : $t('doc-detail-live-disconnected')"
                  ></div>

                  <!-- Last updated -->
                  <span v-if="document.created_by && document.updated_at" class="text-subtle">&middot;</span>
                  <div v-if="document.updated_at" class="flex items-center gap-1.5">
                    <span>{{ formatDate(document.updated_at || new Date().toISOString()) }}</span>
                  </div>
                </div>

                <!-- Action Buttons -->
                <div class="flex items-center gap-2">
                  <!-- Revision History Toggle -->
                  <button
                    @click="toggleRevisionHistory"
                    class="px-3 py-1.5 text-xs rounded-md hover:bg-surface-hover transition-colors flex items-center gap-1.5 text-secondary hover:text-primary"
                    :class="{ 'bg-surface-alt text-primary': showRevisionHistory }"
                    :title="$t('doc-detail-history-title')"
                  >
                    <Icon name="clock" />
                    <span>{{ $t('doc-detail-history') }}</span>
                  </button>
                </div>
              </div>
            </div>

            <!-- Editor + linked tickets share a column gap so the
                 footer doesn't sit flush against the prose surface. -->
            <div class="flex flex-col gap-3">
              <CollaborativeEditor
                v-if="docId"
                ref="editorRef"
                v-model="editContent"
                :doc-id="docId"
                :resource-id="pageId"
                :hide-revision-history="true"
                :placeholder="$t('doc-detail-editor-placeholder')"
                @update:modelValue="updateContent"
                class="w-full flex flex-col"
              />

              <PageTicketLinksPanel
                v-if="document.id"
                :page-id="document.id"
                :can-edit="authStore.isTechnician || authStore.isAdmin"
              />
            </div>
          </div>
        </div>

        <!-- Revision history surface. Same responsive treatment
             as the Insights panel: side panel at md+, bottom
             sheet on phone. Mounted whenever there's a current
             page so toggling open doesn't refetch revisions. -->
        <RevisionHistory
          v-if="currentPageId"
          :open="showRevisionHistory"
          type="documentation"
          :document-id="Number(currentPageId)"
          :active-revision-number="activeRevisionNumber"
          class="flex-shrink-0"
          @close="handleCloseRevisionHistory"
          @select-revision="handleSelectRevision"
          @restored="handleRevisionRestored"
        />

        <!-- Insights surface. Side panel at md+, bottom sheet on
             phone — `<ResponsivePanel>` inside picks the layout
             from the viewport. We mount it whenever there's a
             current page so toggling open/close doesn't unmount
             the contributors fetch state. -->
        <DocumentInsightsPanel
          v-if="currentPageId"
          :open="showInsights"
          :page-id="Number(currentPageId)"
          :created-at="document?.created_at ?? null"
          :updated-at="document?.updated_at ?? null"
          :text="insightsText"
          class="flex-shrink-0"
          @close="handleCloseInsights"
        />
      </div>

      <!-- Not Found State -->
      <div v-else class="p-8 text-center text-secondary flex flex-col items-center gap-4">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-16 w-16 text-tertiary mb-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <h2 class="text-xl font-semibold text-primary">{{ $t('doc-detail-not-found-title') }}</h2>
        <p class="text-secondary max-w-md">{{ $t('doc-detail-not-found-body') }}</p>
        <RouterLink to="/documentation" class="mt-4 text-accent hover:text-accent/80">
          {{ $t('doc-detail-not-found-link') }}
        </RouterLink>
      </div>
    </div>

    <!-- Move Document Modal -->
    <MoveDocumentModal
      v-if="showMoveModal"
      :page-id="document?.id || ''"
      :current-parent-id="document?.parent_id || null"
      @close="showMoveModal = false"
      @moved="handlePageMoved"
    />

    <!-- Collection Manager Modal -->
    <CollectionManager
      v-if="showCollectionManager"
      :page-id="Number(document?.id || 0)"
      :current-collection-ids="[]"
      @close="showCollectionManager = false"
      @updated="showCollectionManager = false"
    />

    <!-- Page Permissions Modal -->
    <PagePermissionsModal
      v-if="showPermissionsModal"
      :page-id="Number(document?.id || 0)"
      @close="showPermissionsModal = false"
      @updated="showPermissionsModal = false"
    />

    <!-- Success message toast -->
    <div
      v-if="showSuccessMessage"
      class="fixed bottom-4 right-4 bg-status-success text-white px-4 py-2 rounded-md shadow-lg flex items-center gap-2 animate-fadeIn"
    >
      <Icon name="checkCircle" size="md" />
      {{ saveMessage }}
    </div>
  </div>
</template>

<style scoped>
@keyframes fadeIn {
  from { opacity: 0; }
  to { opacity: 1; }
}

.animate-fadeIn {
  animation: fadeIn 0.2s ease-out forwards;
}

/* Give the editor a tall minimum so it feels like a full document page.
   Content longer than this grows naturally and scrolls as normal. */
:deep(.collaborative-editor .editor-wrapper) {
  min-height: auto;
}

:deep(.collaborative-editor .editor-container) {
  min-height: auto;
}

:deep(.collaborative-editor .ProseMirror) {
  min-height: max(200px, calc(100vh - 20rem));
  padding-bottom: 25vh;
}
</style>
