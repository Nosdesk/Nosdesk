import { ref, shallowRef, computed, onMounted, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import documentationService from '@/services/documentationService'
import type { Page } from '@/services/documentationService'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import { docsEmitter } from '@/services/docsEmitter'
import { useSSE } from '@/services/sseService'
import { useAuthStore } from '@/stores/auth'

// SSE documentation event data shape
interface DocumentationEventData {
  document_id?: number
  field?: string
  value?: string
  updated_by?: string
  data?: DocumentationEventData
}

/**
 * Shared documentation composable for loading pages and SSE updates
 */
export function useDocumentation() {
  const router = useRouter()
  const documentationNavStore = useDocumentationNavStore()
  const authStore = useAuthStore()

  // Use shallowRef for large page arrays - avoids deep reactivity overhead
  const pages = shallowRef<Page[]>([])
  const pageParentMap = ref<Record<string, string | null>>({})

  // Loading states - differentiated for better UX
  const loading = ref(false)           // Initial load - show skeleton
  const isBackgroundRefresh = ref(false) // Refetching with existing data

  // SSE for real-time updates
  const { addEventListener, removeEventListener, connect, isConnected, isConnecting } = useSSE()

  /**
   * Computed: Show skeleton only when loading with no cached data
   */
  const showSkeleton = computed(() => loading.value && pages.value.length === 0)

  /**
   * Load all documentation pages for the index view
   */
  const loadAllPages = async () => {
    const hasExistingData = pages.value.length > 0

    if (hasExistingData) {
      isBackgroundRefresh.value = true
    } else {
      loading.value = true
    }

    try {
      const topLevelPages = await documentationService.getPages()

      // Build parent-child relationship maps
      const parentMap: Record<string, string | null> = {}
      const hierarchyMap: Record<string, string[]> = {}

      const buildMaps = (page: Page, parentId: string | null = null) => {
        const pageId = String(page.id)
        parentMap[pageId] = parentId

        if (!hierarchyMap[pageId]) {
          hierarchyMap[pageId] = []
        }

        if (page.children && page.children.length > 0) {
          page.children.forEach(child => {
            const childId = String(child.id)
            hierarchyMap[pageId].push(childId)
            buildMaps(child as Page, pageId)
          })
        }
      }

      topLevelPages.forEach(page => buildMaps(page))

      // Update state - use assignment to trigger shallowRef reactivity
      pageParentMap.value = parentMap
      pages.value = topLevelPages

      // Update store with hierarchy
      documentationNavStore.updatePageHierarchy(hierarchyMap)
    } catch (error) {
      console.error('Error loading pages:', error)
    } finally {
      loading.value = false
      isBackgroundRefresh.value = false
    }
  }

  /**
   * Create a new documentation page
   */
  const createNewPage = async () => {
    try {
      const newPageData = {
        title: 'New Documentation Page',
        content: '# New Documentation Page\n\nStart writing your documentation here...',
        description: 'Add a description here',
        status: 'draft',
        icon: '📄',
        slug: 'new-documentation-page-' + Date.now(),
      }

      const newPage = await documentationService.createArticle(newPageData)

      if (newPage?.id) {
        docsEmitter.emit('doc:created', { id: newPage.id })
        documentationNavStore.refreshPages()
        router.push(`/documentation/${newPage.id}`)
        return newPage
      }

      throw new Error('Failed to create new page')
    } catch (error) {
      console.error('Error creating new page:', error)
      throw error
    }
  }

  /**
   * Delete a documentation page
   */
  const deletePage = async (pageId: number | string) => {
    try {
      const success = await documentationService.deleteArticle(pageId)

      if (success) {
        docsEmitter.emit('doc:deleted', { id: pageId })
        documentationNavStore.refreshPages()
        return true
      }

      return false
    } catch (error) {
      console.error('Error deleting page:', error)
      throw error
    }
  }

  /**
   * Setup SSE listener for documentation updates
   */
  const setupSSE = (onUpdate?: (data: DocumentationEventData) => void) => {
    const handleDocumentationUpdate = (event: unknown) => {
      const rawEvent = event as DocumentationEventData
      const data = rawEvent.data || rawEvent

      // Call custom handler if provided
      if (onUpdate) {
        onUpdate(data)
      }
    }

    connect()
    addEventListener('documentation-updated', handleDocumentationUpdate)

    // Return cleanup function
    return () => {
      removeEventListener('documentation-updated' as any, handleDocumentationUpdate)
    }
  }

  return {
    // State
    pages,
    pageParentMap,
    loading,
    isBackgroundRefresh,
    showSkeleton,

    // SSE state
    isConnected,
    isConnecting,

    // Actions
    loadAllPages,
    createNewPage,
    deletePage,
    setupSSE,

    // Store access
    documentationNavStore,
  }
}
