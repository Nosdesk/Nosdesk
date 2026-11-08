/**
 * Starts a documentation page's saved copy (`utils/collabSeed`) as early as
 * whatever opens the page knows which page it is.
 *
 * A route names a page by its slug (or its id), and the server's page
 * request is what turns that into the uuid the saved copy is fetched by. The
 * docs pool usually knows the page already, so the copy starts alongside the
 * page request instead of a round trip behind it.
 */
import { useSyncDocsStore } from '@nosdesk/core/sync/stores/documentation'
import { buildCollabDocId } from '@nosdesk/core/utils/collabDocId'
import { useCollabSessionStore } from '@/stores/collabSession'
import { useMyWorkspacesStore } from '@/stores/myWorkspaces'
import { prefetchSavedCopy } from './collabSeed'

/**
 * The uuid of the page a documentation path names, when the docs pool holds
 * it. A numeric path is the page's id and anything else its slug, as
 * `getPageByPath` reads it.
 */
export function pageUuidForPath(path: string): string | null {
  if (!path) return null
  const pages = useSyncDocsStore().allPages
  const page = !isNaN(Number(path))
    ? pages.find((p) => p.id === Number(path))
    : pages.find((p) => p.slug === path)
  return page?.uuid ?? null
}

/**
 * Start fetching a page's saved copy for its editor. Not when this tab
 * already holds a session for the page: the editor reopens that session with
 * its document and takes no copy, so one fetched now would go unused.
 */
export function prefetchPageBody(pageUuid: string): void {
  if (!hasSession(pageUuid)) prefetchSavedCopy(pageUuid)
}

function hasSession(pageUuid: string): boolean {
  const workspaceUuid = useMyWorkspacesStore().activeWorkspace?.workspace_uuid
  if (!workspaceUuid) return false
  let docId: string
  try {
    docId = buildCollabDocId(workspaceUuid, 'doc', pageUuid)
  } catch {
    // Not a page the editor could open a session for.
    return false
  }
  return useCollabSessionStore().peek(docId) !== null
}
