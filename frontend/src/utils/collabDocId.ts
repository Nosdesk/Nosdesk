/**
 * Workspace-namespaced docId construction + parsing for the
 * collaborative-editor / Yjs layer.
 *
 * # Why the namespace exists
 *
 * Before this helper, collab docIds were the bare resource handle —
 * `ticket-123`, `doc-7`, `collection-4`. That format had two
 * load-bearing assumptions that don't actually hold:
 *
 *   1. **The numeric ID uniquely identifies a logical document
 *      forever.** It doesn't. A `make clean` or a manual database
 *      reset wipes the row and recycles its auto-increment, so the
 *      next ticket created under id 123 is a completely different
 *      document than the previous one — but the y-indexeddb cache
 *      keyed on `ticket-123` happily merges its (now stale) Yjs
 *      updates into the new document on first open, repopulating
 *      notes from a ticket that no longer exists. That's the
 *      "deleted notes came back" bug.
 *   2. **Workspaces don't share IDs.** They do — `ticket-99` is a
 *      perfectly valid id in both workspace A and workspace B
 *      under hosted multi-tenancy. With unprefixed docIds, a tab
 *      that switches workspaces (or a misconfigured tenant
 *      middleware) can route a WS connection to the wrong tenant's
 *      Yjs document.
 *
 * # The namespace
 *
 * Every docId is now `ws-{workspace_uuid}_{kind}-{id}` where
 * `workspace_uuid` is the UUID column on the workspaces table.
 * That UUID is unique per workspace instance — generated fresh
 * when a workspace row is created, replaced by a different UUID
 * whenever the row is recreated (database reset, migration that
 * rewrites the bootstrap workspace, etc.). So:
 *
 *   * Stale IndexedDB caches under the previous workspace UUID
 *     are orphaned (no future code path constructs that docId
 *     again), and get pruned by the existing LRU-by-touched-at
 *     bookkeeping in `useCollabSessionStore`.
 *   * The server validates the prefix against the request's
 *     `WorkspaceContext.workspace_uuid` before opening the doc,
 *     so a stale frontend asking for the wrong tenant's docId
 *     fails fast with 403 instead of silently merging across
 *     tenants.
 *
 * # The format
 *
 *   * `ws-` literal prefix — so the backend parser can detect
 *     namespaced docIds and reject legacy bare ids with a typed
 *     error rather than silently treating them as workspace-1
 *     content.
 *   * Workspace UUID in canonical lowercase hyphenated form (the
 *     same shape `uuid::Uuid::to_string()` and `serde_uuid` emit).
 *   * Single `_` separator between the namespace and the resource
 *     handle. UUIDs never contain `_` and the resource handles
 *     `ticket-N`, `doc-N`, `collection-N` don't either, so the
 *     split is unambiguous.
 *   * Resource handle preserved verbatim — the backend's existing
 *     `DocumentType::from_doc_id` parser keeps working after the
 *     prefix is stripped, no second migration.
 */

/**
 * The three document kinds that flow through the collab WS today.
 * Bounded set; new kinds added to the backend must also land here
 * (and in `collaboration.rs::DocumentType`) so the namespace and
 * parser stay in lockstep.
 */
export type CollabDocKind = 'ticket' | 'doc' | 'collection'

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/

/**
 * Construct a namespaced collab docId for the given workspace +
 * resource. Throws if `workspaceUuid` doesn't match the canonical
 * UUID shape — that's a bug in the caller, not a recoverable
 * runtime case, and we'd rather fail loud than emit a malformed
 * docId the backend would 403 on anyway.
 */
export function buildCollabDocId(
  workspaceUuid: string,
  kind: CollabDocKind,
  id: number | string,
): string {
  if (!UUID_PATTERN.test(workspaceUuid)) {
    throw new Error(
      `buildCollabDocId: workspaceUuid is not a canonical UUID (got "${workspaceUuid}")`,
    )
  }
  return `ws-${workspaceUuid}_${kind}-${id}`
}

/**
 * Inverse of {@link buildCollabDocId}: split a namespaced docId
 * back into its workspace UUID + kind + resource handle. Returns
 * `null` for unparseable input (including legacy bare ids), which
 * is enough information for the caller to surface a clear error
 * without needing typed exceptions for every shape failure.
 */
export function parseCollabDocId(
  docId: string,
): { workspaceUuid: string; kind: CollabDocKind; id: string } | null {
  const match = docId.match(/^ws-([0-9a-f-]{36})_(ticket|doc|collection)-(.+)$/)
  if (!match) return null
  const [, workspaceUuid, kind, id] = match
  if (!UUID_PATTERN.test(workspaceUuid)) return null
  return { workspaceUuid, kind: kind as CollabDocKind, id }
}
