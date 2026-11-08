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
 * Start fetching a page's saved copy for its editor. Not when this tab holds a
 * session for the page that is following its room, or has a copy on its way:
 * the editor reopens that session and takes no copy, so one fetched now would
 * go unused. A session whose connection lapsed takes one, as it holds the page
 * as it was when it lapsed.
 */
export function prefetchPageBody(pageUuid: string): void {
  if (needsCopy(pageUuid)) prefetchSavedCopy(pageUuid)
}

function needsCopy(pageUuid: string): boolean {
  const workspaceUuid = useMyWorkspacesStore().activeWorkspace?.workspace_uuid
  if (!workspaceUuid) return true
  let docId: string
  try {
    docId = buildCollabDocId(workspaceUuid, 'doc', pageUuid)
  } catch {
    // Not a page the editor could open a session for.
    return true
  }
  return useCollabSessionStore().needsSavedCopy(docId)
}

/**
 * Hover-prefetch a page's collaborative session, as ticket rows do: the
 * WebSocket, the local cache and the saved copy are on their way before the
 * click. The session disconnects on its own after the grace window if the
 * page isn't opened. For any link to a page: pass its uuid, or its id for
 * the docs pool to name it. Skipped while the workspace or the page's uuid is
 * unknown.
 */
export function warmPage(page: { uuid?: string | null; id?: number | string | null }): void {
  const workspaceUuid = useMyWorkspacesStore().activeWorkspace?.workspace_uuid
  if (!workspaceUuid) return
  const id = page.id == null ? NaN : Number(page.id)
  const pageUuid =
    page.uuid ?? (Number.isFinite(id) ? useSyncDocsStore().allPages.find((p) => p.id === id)?.uuid : undefined)
  if (!pageUuid) return
  let docId: string
  try {
    docId = buildCollabDocId(workspaceUuid, 'doc', pageUuid)
  } catch {
    return
  }
  useCollabSessionStore().warm(docId)
}
