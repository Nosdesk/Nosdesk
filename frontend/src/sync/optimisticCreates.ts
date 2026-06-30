import { reactive } from 'vue'

/**
 * Structural reconcile for optimistic comment creates.
 *
 * The client mints a correlation id (UUID), sends it on the create POST, and the
 * server echoes it on the `comment.created` sync action (backend stamps
 * `client_id` -> `correlation_id`). While a create is pending:
 *
 *  - the optimistic temp row keeps rendering (instant, with its local blobs),
 *  - the server's echoed row is SUPPRESSED from the view. The sync stream
 *    delivers `comment.created` and its `attachment.created`s separately, so the
 *    echo arrives attachment-less for a beat; suppressing it stops that empty/
 *    duplicate flash beside the optimistic bubble (most visible for voice notes,
 *    whose slow upload lets the echo beat the REST reply).
 *
 * The REST reply does the actual temp -> real swap (it carries the attachments)
 * and `clear`s the entry, un-suppressing the now-complete row. If the REST reply
 * fails but the echo arrived, clearing still un-suppresses so the server's row
 * stands (the comment really was created). Match is by id, never a heuristic.
 */
const pendingRealId = new Map<string, number | null>()
const suppressed = reactive(new Set<number>())

/** Begin tracking a pending optimistic create keyed by its correlation id. */
export function registerOptimisticCreate(correlationId: string): void {
  pendingRealId.set(correlationId, null)
}

/** Note the server echo of a pending create; hide its row until the swap. */
export function noteServerEcho(correlationId: string, realId: number): void {
  if (pendingRealId.has(correlationId)) {
    pendingRealId.set(correlationId, realId)
    suppressed.add(realId)
  }
}

/** The REST reply reconciled the temp itself: drop the entry + un-suppress. */
export function clearOptimisticCreate(correlationId: string): void {
  const realId = pendingRealId.get(correlationId)
  pendingRealId.delete(correlationId)
  if (realId != null) suppressed.delete(realId)
}

/** Is this row a suppressed server echo of a still-pending optimistic create? */
export function isEchoSuppressed(id: number): boolean {
  return suppressed.has(id)
}
