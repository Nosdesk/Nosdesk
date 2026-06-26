import { useRouter } from 'vue-router'
import documentationService from '@/services/documentationService'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import { docUrl } from '@nosdesk/core/utils/docUrl'
import { docsEmitter } from '@/services/docsEmitter'

/**
 * Documentation mutations + shared nav store access.
 *
 * Reads (page lists, trees, collections) now derive from the sync pool
 * via `useDocPages` / `useSyncDocsStore`; this composable is only the
 * create / delete mutations. Live metadata flows through the pool; the
 * document view's live-connection indicator reads the collab session
 * store directly (the Yjs WS), not SSE.
 */
export function useDocumentation() {
  const router = useRouter()
  const documentationNavStore = useDocumentationNavStore()

  /**
   * Create a new documentation page and navigate to it. The page lands
   * in the pool via its created sync event, so the nav + index update
   * themselves.
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
        router.push(docUrl(newPage))
        return newPage
      }

      throw new Error('Failed to create new page')
    } catch (error) {
      console.error('Error creating new page:', error)
      throw error
    }
  }

  /**
   * Soft-delete a documentation page (moves it to trash). The status
   * change flows back through the pool, so the lists reconcile.
   */
  const deletePage = async (pageId: number | string) => {
    try {
      const success = await documentationService.deleteArticle(pageId)

      if (success) {
        docsEmitter.emit('doc:deleted', { id: pageId })
        return true
      }

      return false
    } catch (error) {
      console.error('Error deleting page:', error)
      throw error
    }
  }

  return {
    // Actions
    createNewPage,
    deletePage,

    // Store access
    documentationNavStore,
  }
}
