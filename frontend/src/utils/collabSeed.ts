/**
 * The saved copy of a documentation page's collaborative document, for the
 * editor's first paint.
 *
 * A page this browser hasn't opened before has nothing in its local cache, so
 * its editor stayed empty until the room's WebSocket had a token, connected
 * and synced (about a second on hosted). The page's saved document is served
 * over REST too; fetched alongside the route's page preload and merged into
 * the editor's document as soon as it arrives, the body paints with the
 * title. The room's sync then merges the rest, as it does over the local
 * cache: the room only ever adds edits to the same document (a revision
 * restore is new operations, never a replacement), so a slightly older copy
 * merges without duplicating anything.
 *
 * The request declares this editor's schema, as the room's connection does,
 * and the server withholds a document that needs a newer editor, which this
 * one would delete content from.
 */
import apiClient from '@nosdesk/core/apiClient'
import { EDITOR_SCHEMA_VERSION } from '@/components/editor/schemaVersion'

/** How long a prefetched copy waits to be taken before it's dropped. */
const UNCLAIMED_MS = 30_000

const pending = new Map<string, { copy: Promise<Uint8Array | null>; timer: ReturnType<typeof setTimeout> }>()

function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return bytes
}

async function fetchSavedCopy(pageUuid: string): Promise<Uint8Array | null> {
  try {
    const { data } = await apiClient.get<{ yjs_document?: string | null }>(
      `/documentation/pages/uuid/${pageUuid}/content`,
      { params: { schema: EDITOR_SCHEMA_VERSION } },
    )
    return data.yjs_document ? base64ToBytes(data.yjs_document) : null
  } catch {
    // The room serves the document anyway; this only made it sooner.
    return null
  }
}

/** Start fetching a page's saved copy, for the editor to take when it opens. */
export function prefetchSavedCopy(pageUuid: string): void {
  if (pending.has(pageUuid)) return
  const timer = setTimeout(() => pending.delete(pageUuid), UNCLAIMED_MS)
  pending.set(pageUuid, { copy: fetchSavedCopy(pageUuid), timer })
}

/** A page's saved copy: the prefetched one if there is one, else fetched now. */
export function takeSavedCopy(pageUuid: string): Promise<Uint8Array | null> {
  const prefetched = pending.get(pageUuid)
  if (prefetched) {
    clearTimeout(prefetched.timer)
    pending.delete(pageUuid)
    return prefetched.copy
  }
  return fetchSavedCopy(pageUuid)
}
