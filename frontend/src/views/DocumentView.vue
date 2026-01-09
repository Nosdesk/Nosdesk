<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { formatDate } from '@/utils/dateUtils'
import { useTitleManager } from '@/composables/useTitleManager'
import { useDocumentation } from '@/composables/useDocumentation'
import documentationService from '@/services/documentationService'
import ticketService from '@/services/ticketService'
import type { Article, Page } from '@/services/documentationService'
import CollaborativeEditor from '@/components/CollaborativeEditor.vue'
import BackButton from '@/components/common/BackButton.vue'
import DeleteButton from '@/components/common/DeleteButton.vue'
import RevisionHistory from '@/components/editor/RevisionHistory.vue'
import apiClient from '@/services/apiConfig'
import { useAuthStore } from '@/stores/auth'

const route = useRoute()
const router = useRouter()
const titleManager = useTitleManager()
const authStore = useAuthStore()

// Use shared documentation composable
const {
  deletePage,
  setupSSE,
  documentationNavStore,
  isConnected,
  isConnecting,
} = useDocumentation()

// Document state
const article = ref<Article | null>(null)
const page = ref<Page | null>(null)
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

// Emits
const emit = defineEmits<{
  (e: 'update:title', title: string): void
  (e: 'update:document', document: { id: string; title: string; icon: string; slug?: string } | null): void
}>()

// Document object for header
const documentObj = computed(() => {
  if (page.value) {
    return {
      id: String(page.value.id),
      title: page.value.title,
      icon: page.value.icon || '📄',
      slug: page.value.slug
    }
  } else if (article.value) {
    return {
      id: String(article.value.id),
      title: article.value.title,
      icon: article.value.icon || documentIcon.value,
      slug: article.value.slug
    }
  }
  return null
})

// Doc ID for CollaborativeEditor
const docId = computed(() => {
  if (isTicketNote.value && ticketId.value) {
    return `ticket-${ticketId.value}`
  }
  if (page.value?.ticket_id) {
    return `ticket-${page.value.ticket_id}`
  }
  if (article.value?.ticket_id) {
    return `ticket-${article.value.ticket_id}`
  }
  if (page.value) {
    return `doc-${page.value.id}`
  }
  if (article.value) {
    return `doc-${article.value.id}`
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
  if (article.value) {
    article.value.content = newContent
  } else if (page.value) {
    page.value.content = newContent
  }
}

// Title update handler with debounced save
const updateTitle = (newTitle: string) => {
  editTitle.value = newTitle

  if (article.value || page.value) {
    emit('update:title', newTitle)
    titleManager.setCustomTitle(newTitle)

    const slug = newTitle.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '')

    if (article.value) {
      article.value.title = newTitle
      article.value.slug = slug
    } else if (page.value) {
      page.value.title = newTitle
      page.value.slug = slug
    }

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
  if (!page.value && !article.value) return

  const pageId = page.value?.id || article.value?.id
  if (!pageId) return

  const newSlug = editTitle.value.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '')

  try {
    await apiClient.put(`/documentation/pages/${pageId}`, {
      title: editTitle.value,
      slug: newSlug,
    })

    documentationNavStore.updatePageField(pageId, 'title', editTitle.value)
    documentationNavStore.updatePageField(pageId, 'slug', newSlug)
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
    const pageId = page.value?.id || article.value?.id
    if (!pageId) return

    const response = await apiClient.get(
      `/collaboration/docs/${pageId}/revisions/${revisionNumber}`
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
  if (!article.value && !page.value) return

  try {
    isSaving.value = true
    saveMessage.value = 'Deleting document...'
    showSuccessMessage.value = true

    const pageId = article.value?.id || page.value?.id
    if (pageId) {
      const success = await deletePage(pageId)

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

// Fetch document content
const fetchContent = async () => {
  isLoading.value = true

  // Check for ticket note mode
  if (route.query.ticketId) {
    const ticketIdParam = route.query.ticketId as string

    try {
      const ticket = await ticketService.getTicketById(Number(ticketIdParam))

      if (ticket) {
        article.value = {
          id: `ticket-note-${ticketIdParam}`,
          title: `Notes for Ticket #${ticket.id}`,
          description: `Documentation for ticket ${ticket.title}`,
          content: ticket.article_content || '',
          author: ticket.assignee || 'System',
          lastUpdated: ticket.modified,
          status: 'published',
          slug: '',
          parent_id: null,
          icon: null
        }

        isTicketNote.value = true
        ticketId.value = ticketIdParam
        editContent.value = article.value.content || ''
        editTitle.value = article.value.title
        documentIcon.value = article.value?.icon || 'mdi-text-box-outline'

        emit('update:title', article.value.title)
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
        page.value = result
        editContent.value = page.value.content || ''
        editTitle.value = page.value.title
        documentIcon.value = page.value.icon || 'mdi-folder-outline'
        emit('update:title', page.value.title)
      } else if ('id' in result) {
        const articleData = await documentationService.getArticleById(String(result.id))

        if (articleData) {
          article.value = articleData
          editContent.value = article.value.content || ''
          editTitle.value = article.value.title
          documentIcon.value = article.value?.icon || 'mdi-text-box-outline'
          emit('update:title', article.value.title)
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
  }
}

// SSE handler for real-time updates
const handleSSEUpdate = (data: { document_id?: number; field?: string; value?: string }) => {
  const currentPageId = page.value?.id || article.value?.id

  if (data.document_id === currentPageId) {
    if (data.field === 'title' && data.value) {
      if (page.value) {
        page.value.title = data.value
      } else if (article.value) {
        article.value.title = data.value
      }
      editTitle.value = data.value
      titleManager.setCustomTitle(data.value)
      emit('update:title', data.value)

      if (titleElementRef.value && titleElementRef.value.textContent !== data.value) {
        titleElementRef.value.textContent = data.value
      }
    }
    if (data.field === 'slug' && data.value) {
      if (page.value) {
        page.value.slug = data.value
      } else if (article.value) {
        article.value.slug = data.value
      }
    }
    if (data.field === 'icon' && data.value) {
      if (page.value) {
        page.value.icon = data.value
      } else if (article.value) {
        article.value.icon = data.value
      }
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

        <!-- Delete button -->
        <DeleteButton
          v-if="(article || page) && !isTicketNote"
          :itemName="'documentation page'"
          @delete="handleDeletePage"
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
      <div v-else-if="article || page" class="w-full flex">
        <!-- Main Content Area -->
        <div class="flex-1 flex justify-center">
          <div class="w-full max-w-3xl px-4 sm:px-6 lg:px-8 py-6 sm:py-8 flex flex-col">
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
                  {{ editTitle || (page || article)?.title || 'Untitled' }}
                </h1>
              </div>

              <!-- Metadata bar -->
              <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3 pt-4 pb-2 border-t border-subtle">
                <!-- Metadata -->
                <div class="flex flex-wrap items-center gap-x-3 gap-y-2 text-xs text-tertiary">
                  <!-- Created By -->
                  <div v-if="page?.created_by || article?.created_by" class="flex items-center gap-1.5">
                    <span class="text-secondary">{{ (page || article)?.created_by?.name || 'Unknown' }}</span>
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
                  <span v-if="(page?.created_by || article?.created_by) && (page?.updated_at || article?.updated_at)" class="text-subtle">·</span>

                  <!-- Last Updated -->
                  <div v-if="page?.updated_at || article?.updated_at" class="flex items-center gap-1.5">
                    <span>{{ formatDate((page || article)?.updated_at || new Date().toISOString()) }}</span>
                  </div>

                  <!-- Last Edited By -->
                  <template v-if="page?.last_edited_by || article?.last_edited_by">
                    <span class="text-subtle">·</span>
                    <span>Edited by {{ (page || article)?.last_edited_by?.name || 'Unknown' }}</span>
                  </template>
                </div>

                <!-- Action Buttons -->
                <div class="flex items-center gap-2">
                  <!-- Linked Ticket Button -->
                  <RouterLink
                    v-if="page?.ticket_id || article?.ticket_id"
                    :to="`/tickets/${page?.ticket_id || article?.ticket_id}`"
                    class="px-3 py-1.5 text-xs rounded-md hover:bg-surface-hover transition-colors flex items-center gap-1.5 text-secondary hover:text-primary"
                    title="View linked ticket"
                  >
                    <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 5v2m0 4v2m0 4v2M5 5a2 2 0 00-2 2v3a2 2 0 110 4v3a2 2 0 002 2h14a2 2 0 002-2v-3a2 2 0 110-4V7a2 2 0 00-2-2H5z" />
                    </svg>
                    <span>Ticket #{{ page?.ticket_id || article?.ticket_id }}</span>
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
          v-if="showRevisionHistory && (page?.id || article?.id)"
          type="documentation"
          :document-id="Number(page?.id || article?.id)"
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
</style>
