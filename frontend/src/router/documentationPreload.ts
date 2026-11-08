import type { RouteLocationNormalized } from 'vue-router'
import type { Page, Article } from '@nosdesk/core/services/documentationService'

/**
 * The documentation page route's `beforeEnter`: loads the page so the view
 * renders it at once, and starts the editor's body alongside it.
 */
export async function preloadDocumentationPage(to: RouteLocationNormalized) {
  // Set a generic title initially
  to.meta.titleKey = 'route-title-documentation'
  to.meta.title = undefined
  to.meta.titleKeyArgs = undefined
  to.meta.preloadedDocument = undefined

  // A page that doesn't exist and one the viewer can't open read the
  // same: the not-found page, not the documentation index.
  const docNotFound = { name: 'error', params: { code: '404' }, replace: true } as const

  const path = to.params.path?.toString()
  if (!path) return

  // The editor's body, fetched alongside the rest of the preload so it
  // paints with the title (see utils/collabSeed). The docs pool usually knows
  // the page, so the body starts with the page request rather than after it.
  // Its module brings the collab session store, which a cold start may still
  // be loading, so the page request doesn't wait for it.
  const bodyPrefetch = import('@/utils/docBodyPrefetch')
  void bodyPrefetch
    .then(({ pageUuidForPath, prefetchPageBody }) => {
      const known = pageUuidForPath(path)
      if (known) prefetchPageBody(known)
    })
    .catch(() => {})

  try {
    const { getPageByPath, getArticleById } = await import(
      '@nosdesk/core/services/documentationService'
    )
    const result = await getPageByPath(path)
    if (!result) return docNotFound
    if ('uuid' in result && typeof result.uuid === 'string') {
      const uuid = result.uuid
      void bodyPrefetch.then(({ prefetchPageBody }) => prefetchPageBody(uuid)).catch(() => {})
    }

    let doc: Page | Article = result
    // If the result is a page stub (no children array), fetch full article
    if (!('children' in result && Array.isArray(result.children)) && 'id' in result) {
      const articleData = await getArticleById(String(result.id))
      if (articleData) doc = articleData
      else return docNotFound
    }
    to.meta.preloadedDocument = doc
    // Document title is user content, not translatable. Set as
    // literal `title`; consumers prefer titleKey, so clear it.
    if (doc.title) {
      to.meta.title = doc.title
      to.meta.titleKey = undefined
    }
  } catch {
    return docNotFound
  }
}
