/**
 * Workspace-namespaced docId construction + parsing for the
 * collaborative-editor / Yjs layer.
 *
 * # Why the namespace exists
 *
 * Before this helper, collab docIds were the bare resource handle by
 * integer id — `ticket-123`, `doc-7`. That format had two load-bearing
 * assumptions that don't actually hold:
 *
 *   1. **The numeric ID uniquely identifies a logical document
 *      forever.** It doesn't. A `make clean` or a manual database
 *      reset wipes the row and recycles its auto-increment, so the
 *      next ticket created under id 123 is a completely different
 *      document than the previous one — but a cache keyed on
 *      `ticket-123` happily merges its (now stale) Yjs updates into
 *      the new document on first open, repopulating notes from a
 *      ticket that no longer exists. That's the "deleted notes came
 *      back" bug.
 *   2. **Workspaces don't share IDs.** They do — `ticket-99` is a
 *      perfectly valid id in both workspace A and workspace B
 *      under hosted multi-tenancy.
 *
 * # The fix: workspace namespace + immutable resource UUID
 *
 * Every docId is now `ws-{workspace_uuid}_{kind}-{resource_uuid}`,
 * where both UUIDs are immutable, never-recycled identities:
 *
 *   * **`resource_uuid`** is the resource row's own UUID (tickets,
 *     documentation_pages, collections each carry one). It is minted
 *     once at creation and never reused, so a wiped+recreated ticket
 *     that reuses integer id 123 gets a brand-new UUID and therefore a
 *     brand-new docId. The old cache is unreachable, not merged. This
 *     is what actually fixes assumption #1 — keying on the workspace
 *     UUID alone did not, because a database reset that keeps the
 *     workspace row (only recycling ticket ids) leaves the workspace
 *     UUID unchanged.
 *   * **`workspace_uuid`** bounds the tenant: the server validates it
 *     against the request's `WorkspaceContext.workspace_uuid` before
 *     opening the doc, so a stale frontend asking for the wrong
 *     tenant's docId fails fast instead of merging across tenants.
 *
 * The backend resolves `resource_uuid` back to the integer id its
 * persistence layer uses (see `collaboration.rs`); the durable rows
 * stay keyed by their integer FK, only the doc identity is the UUID.
 *
 * # The format
 *
 *   * `ws-` literal prefix so the backend parser can detect namespaced
 *     docIds and reject legacy bare/integer forms with a typed error.
 *   * Workspace UUID in canonical lowercase hyphenated form.
 *   * Single `_` separator between the namespace and the resource
 *     handle. UUIDs never contain `_`, so the split is unambiguous
 *     even though the resource UUID itself contains `-`.
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
  resourceUuid: string,
): string {
  if (!UUID_PATTERN.test(workspaceUuid)) {
    throw new Error(
      `buildCollabDocId: workspaceUuid is not a canonical UUID (got "${workspaceUuid}")`,
    )
  }
  if (!UUID_PATTERN.test(resourceUuid)) {
    throw new Error(
      `buildCollabDocId: resourceUuid is not a canonical UUID (got "${resourceUuid}")`,
    )
  }
  return `ws-${workspaceUuid}_${kind}-${resourceUuid}`
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
