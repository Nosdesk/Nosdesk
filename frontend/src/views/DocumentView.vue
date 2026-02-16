<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { formatDate } from '@/utils/dateUtils'
import { slugify } from '@/utils/docUrl'
import { useTitleManager } from '@/composables/useTitleManager'
import { useDocumentation } from '@/composables/useDocumentation'
import { useClipboard } from '@/composables/useClipboard'
import documentationService from '@/services/documentationService'
import ticketService from '@/services/ticketService'
import type { Page } from '@/services/documentationService'
import CollaborativeEditor from '@/components/CollaborativeEditor.vue'
import BackButton from '@/components/common/BackButton.vue'
import DocumentActionsMenu from '@/components/documentationComponents/DocumentActionsMenu.vue'
import MoveDocumentModal from '@/components/documentationComponents/MoveDocumentModal.vue'
import DocumentationBreadcrumb from '@/components/documentationComponents/DocumentationBreadcrumb.vue'
import CollectionManager from '@/components/documentationComponents/CollectionManager.vue'
import PagePermissionsModal from '@/components/documentationComponents/PagePermissionsModal.vue'
import { docsEmitter } from '@/services/docsEmitter'
import RevisionHistory from '@/components/editor/RevisionHistory.vue'
import apiClient from '@/services/apiConfig'
import { useAuthStore } from '@/stores/auth'
import { useDocumentationNavStore } from '@/stores/documentationNav'

const route = useRoute()
const router = useRouter()
const titleManager = useTitleManager()
const authStore = useAuthStore()
const docNavStore = useDocumentationNavStore()
const { copied: copiedLink, copy: copyToClipboard } = useClipboard()

// Use shared documentation composable
const {
  deletePage,
  setupSSE,
  documentationNavStore,
  isConnected,
  isConnecting,
} = useDocumentation()

// Document state — single ref replaces the old article + page dual refs
const document = ref<Page | null>(null)
const isLoading = ref(true)
const isSaving = ref(false)
const saveMessage = ref('')
const showSuccessMessage = ref(false)

// Content editing
const editContent = ref('')
const editTitle = ref('')
const documentIcon = ref('📄')

// Ref for the title h1 element
const titleElementRef = ref<HTMLElement | null>(null)

// Debounced title save
let titleUpdateTimeout: ReturnType<typeof setTimeout> | null = null

// Revision history
const showRevisionHistory = ref(false)
const editorRef = ref<InstanceType<typeof CollaborativeEditor> | null>(null)

// Ticket note mode
const isTicketNote = ref(false)
const ticketId = ref<string | null>(null)

// Subscription state
const isSubscribed = ref(false)

// Starred state
const isStarred = ref(false)

// Computed helpers
const currentPageId = computed(() => document.value?.id ?? null)
const isDocumentPage = computed(() => !!document.value && !isTicketNote.value)

const handleCopyLink = () => {
  const slug = document.value?.slug || document.value?.id
  const url = `${window.location.origin}/documentation/${slug}`
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

// Doc ID for CollaborativeEditor
const docId = computed(() => {
  if (isTicketNote.value && ticketId.value) {
    return `ticket-${ticketId.value}`
  }
  if (document.value?.ticket_id) {
    return `ticket-${document.value.ticket_id}`
  }
  if (document.value) {
    return `doc-${document.value.id}`
  }
  return 'documentation-new'
})

// Navigation helpers
const fallbackRoute = computed(() => {
  if (isTicketNote.value && ticketId.value) {
    return `/tickets/${ticketId.value}`
  }
  return '/documentation'
})

const backButtonLabel = computed(() => {
  if (isTicketNote.value) {
    return 'Back to Ticket'
  }
  return 'Back to Documentation'
})

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
    await apiClient.put(`/documentation/pages/${currentPageId.value}`, {
      title: editTitle.value,
      slug: newSlug,
    })

    documentationNavStore.updatePageField(currentPageId.value, 'title', editTitle.value)
    documentationNavStore.updatePageField(currentPageId.value, 'slug', newSlug)
  } catch (error) {
    console.error('Failed to save title:', error)
  }
}

// Revision history handlers
const toggleRevisionHistory = () => {
  showRevisionHistory.value = !showRevisionHistory.value
}

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
  showRevisionHistory.value = false
  if (editorRef.value?.isViewingRevision) {
    editorRef.value.exitRevisionView()
  }
}

const handleRevisionRestored = () => {
  fetchContent()
}

// Delete handler
const handleDeletePage = async () => {
  if (!document.value) return

  try {
    isSaving.value = true
    saveMessage.value = 'Deleting document...'
    showSuccessMessage.value = true

    const success = await deletePage(document.value.id)

    if (success) {
      saveMessage.value = 'Document deleted successfully'
      setTimeout(() => {
        router.push('/documentation')
      }, 1000)
    } else {
      saveMessage.value = 'Error deleting document'
      setTimeout(() => {
        showSuccessMessage.value = false
      }, 3000)
    }
  } catch (error) {
    console.error('Error deleting page:', error)
    saveMessage.value = 'Error deleting document'
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
    const response = await apiClient.get(`/documentation/pages/${currentPageId.value}/export/markdown`, {
      responseType: 'blob',
    })
    const blob = new Blob([response.data], { type: 'text/markdown' })
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
      await apiClient.post(`/documentation/pages/${currentPageId.value}${opts.apiSuffix}`)
    } else {
      await apiClient.put(`/documentation/pages/${currentPageId.value}`, { status })
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
      title: `${document.value.title} (copy)`,
      content: document.value.content || '',
      description: document.value.description || '',
      status: 'draft',
      icon: document.value.icon || '📄',
    })

    if (newPage?.id) {
      docsEmitter.emit('doc:created', { id: newPage.id })
      documentationNavStore.refreshPages()
      router.push(`/documentation/${newPage.slug || newPage.id}`)
    }
  } catch (error) {
    console.error('Failed to duplicate page:', error)
  }
}

// Fetch document content
const fetchContent = async () => {
  isLoading.value = true

  // Check for ticket note mode
  if (route.query.ticketId) {
    const ticketIdParam = route.query.ticketId as string

    try {
      const ticket = await ticketService.getTicketById(Number(ticketIdParam))

      if (ticket) {
        document.value = {
          id: `ticket-note-${ticketIdParam}`,
          title: `Notes for Ticket #${ticket.id}`,
          description: `Documentation for ticket ${ticket.title}`,
          content: ticket.article_content || '',
          author: ticket.assignee || 'System',
          lastUpdated: ticket.modified,
          status: 'published',
          slug: '',
          parent_id: null,
          icon: null,
          children: [],
        }

        isTicketNote.value = true
        ticketId.value = ticketIdParam
        editContent.value = document.value.content || ''
        editTitle.value = document.value.title
        documentIcon.value = document.value.icon || 'mdi-text-box-outline'

        emit('update:title', document.value.title)
        isLoading.value = false
        return
      }
    } catch (error) {
      console.error(`Error loading ticket ${ticketIdParam}:`, error)
    }
  }

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
    if (currentPageId.value && !isTicketNote.value) {
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

// SSE handler for real-time updates
const handleSSEUpdate = (data: { document_id?: number; field?: string; value?: string }) => {
  if (data.document_id === currentPageId.value) {
    if (data.field === 'title' && data.value) {
      if (document.value) document.value.title = data.value
      editTitle.value = data.value
      titleManager.setCustomTitle(data.value)
      emit('update:title', data.value)

      if (titleElementRef.value && titleElementRef.value.textContent !== data.value) {
        titleElementRef.value.textContent = data.value
      }
    }
    if (data.field === 'slug' && data.value) {
      if (document.value) document.value.slug = data.value
    }
    if (data.field === 'icon' && data.value) {
      if (document.value) document.value.icon = data.value
      documentIcon.value = data.value
    }
  }
}

// Lifecycle
let cleanupSSE: (() => void) | null = null

onMounted(() => {
  cleanupSSE = setupSSE(handleSSEUpdate)
  fetchContent()
})

onUnmounted(() => {
  if (cleanupSSE) {
    cleanupSSE()
  }
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
    <!-- Header bar -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2 flex-wrap">
        <!-- Back button -->
        <BackButton :fallbackRoute="fallbackRoute" :label="backButtonLabel" />

        <!-- Spacer -->
        <div class="flex-1"></div>

        <!-- Saving indicator -->
        <span v-if="isSaving" class="text-accent flex items-center gap-1 text-xs">
          <svg class="animate-spin h-3 w-3" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          Saving...
        </span>

        <!-- Publish button for unpublished pages -->
        <button
          v-if="document && !isTicketNote && document.status !== 'published'"
          @click="handlePublishPage"
          class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md bg-emerald-600 text-white hover:bg-emerald-700 transition-colors"
        >
          <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
          </svg>
          <span class="hidden sm:inline">Publish</span>
        </button>

        <!-- Star button -->
        <button
          v-if="isDocumentPage"
          @click="isStarred ? handleUnstar() : handleStar()"
          class="p-1.5 rounded-md hover:bg-surface-hover transition-colors"
          :class="isStarred ? 'text-amber-500' : 'text-secondary hover:text-primary'"
          :title="isStarred ? 'Unstar page' : 'Star page'"
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
          :title="copiedLink ? 'Copied!' : 'Copy link'"
        >
          <svg v-if="!copiedLink" class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1" />
          </svg>
          <svg v-else class="w-5 h-5 text-emerald-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
          </svg>
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
        />
      </div>
    </div>

    <!-- Main content -->
    <div class="flex flex-col flex-1 overflow-auto bg-gradient-to-b from-bg-app to-bg-surface items-center">
      <!-- Loading state -->
      <div v-if="isLoading" class="flex items-center justify-center h-full">
        <div class="animate-spin h-8 w-8 border-2 border-accent border-t-transparent rounded-full"></div>
      </div>

      <!-- Document Content View -->
      <div v-else-if="document" class="w-full flex">
        <!-- Main Content Area -->
        <div class="flex-1 flex justify-center">
          <div class="w-full max-w-3xl px-4 sm:px-6 lg:px-8 py-6 sm:py-8 flex flex-col">
            <!-- Breadcrumb -->
            <DocumentationBreadcrumb
              v-if="!isTicketNote"
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
                  {{ editTitle || document.title || 'Untitled' }}
                </h1>
              </div>

              <!-- Metadata bar -->
              <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3 pt-4 pb-2 border-t border-subtle">
                <!-- Metadata -->
                <div class="flex flex-wrap items-center gap-x-3 gap-y-2 text-xs text-tertiary">
                  <!-- Status Badge -->
                  <span v-if="document.status === 'draft'" class="text-[10px] px-1.5 py-0.5 rounded bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300 font-medium">Draft</span>
                  <span v-else-if="document.status === 'archived'" class="text-[10px] px-1.5 py-0.5 rounded bg-gray-100 text-gray-600 dark:bg-gray-700/50 dark:text-gray-300 font-medium">Archived</span>

                  <!-- Created By -->
                  <div v-if="document.created_by" class="flex items-center gap-1.5">
                    <span class="text-secondary">{{ document.created_by?.name || 'Unknown' }}</span>
                  </div>

                  <!-- SSE Connection Status -->
                  <div
                    class="w-2 h-2 rounded-full flex-shrink-0"
                    :class="{
                      'bg-status-success animate-pulse': isConnected,
                      'bg-status-warning animate-pulse': isConnecting && !isConnected,
                      'bg-status-error': !isConnected && !isConnecting,
                    }"
                    :title="isConnected ? 'Live updates active' : isConnecting ? 'Connecting...' : 'Disconnected'"
                  ></div>

                  <!-- Separator -->
                  <span v-if="document.created_by && document.updated_at" class="text-subtle">&middot;</span>

                  <!-- Last Updated -->
                  <div v-if="document.updated_at" class="flex items-center gap-1.5">
                    <span>{{ formatDate(document.updated_at || new Date().toISOString()) }}</span>
                  </div>

                  <!-- Last Edited By -->
                  <template v-if="document.last_edited_by">
                    <span class="text-subtle">&middot;</span>
                    <span>Edited by {{ document.last_edited_by?.name || 'Unknown' }}</span>
                  </template>
                </div>

                <!-- Action Buttons -->
                <div class="flex items-center gap-2">
                  <!-- Linked Ticket Button -->
                  <RouterLink
                    v-if="document.ticket_id"
                    :to="`/tickets/${document.ticket_id}`"
                    class="px-3 py-1.5 text-xs rounded-md hover:bg-surface-hover transition-colors flex items-center gap-1.5 text-secondary hover:text-primary"
                    title="View linked ticket"
                  >
                    <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 5v2m0 4v2m0 4v2M5 5a2 2 0 00-2 2v3a2 2 0 110 4v3a2 2 0 002 2h14a2 2 0 002-2v-3a2 2 0 110-4V7a2 2 0 00-2-2H5z" />
                    </svg>
                    <span>Ticket #{{ document.ticket_id }}</span>
                  </RouterLink>

                  <!-- Revision History Toggle -->
                  <button
                    @click="toggleRevisionHistory"
                    class="px-3 py-1.5 text-xs rounded-md hover:bg-surface-hover transition-colors flex items-center gap-1.5 text-secondary hover:text-primary"
                    :class="{ 'bg-surface-alt text-primary': showRevisionHistory }"
                    title="Revision history"
                  >
                    <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                    </svg>
                    <span>History</span>
                  </button>
                </div>
              </div>
            </div>

            <!-- Editor -->
            <CollaborativeEditor
              ref="editorRef"
              v-model="editContent"
              :doc-id="docId"
              :hide-revision-history="true"
              placeholder="Enter documentation content here..."
              @update:modelValue="updateContent"
              class="w-full flex-1 flex flex-col"
            />
          </div>
        </div>

        <!-- Revision History Sidebar -->
        <RevisionHistory
          v-if="showRevisionHistory && currentPageId"
          type="documentation"
          :document-id="Number(currentPageId)"
          class="flex-shrink-0"
          @close="handleCloseRevisionHistory"
          @select-revision="handleSelectRevision"
          @restored="handleRevisionRestored"
        />
      </div>

      <!-- Not Found State -->
      <div v-else class="p-8 text-center text-secondary flex flex-col items-center gap-4">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-16 w-16 text-tertiary mb-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <h2 class="text-xl font-semibold text-primary">Document not found</h2>
        <p class="text-secondary max-w-md">The document you're looking for doesn't exist or has been moved.</p>
        <RouterLink to="/documentation" class="mt-4 text-accent hover:text-accent/80">
          Go to Documentation Home
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
      <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
        <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd" />
      </svg>
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
