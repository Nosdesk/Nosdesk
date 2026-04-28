/**
 * Ticket-deletion cleanup.
 *
 * Server-authoritative SSE: when the server announces
 * `ticket-deleted`, wipe every local artefact for that ticket so
 * a future open or refresh doesn't surface stale data the user
 * shouldn't see again. Three artefacts:
 *
 *   - Yjs collab session (if active in this tab) and its on-disk
 *     IndexedDB cache, via `useCollabSessionStore.purgeData`.
 *   - In-progress comment draft text/internal flag (localStorage)
 *     via `useTicketDraftsStore.clearDraft`.
 *   - In-memory pending attachments + plugin-activation counters
 *     via `useTicketUiStore.clear*`.
 *
 * Registered once at app boot from `App.vue` so the listener
 * lives for the entire session, regardless of which view is
 * currently mounted. Without this, a deleted ticket's draft
 * would persist in localStorage indefinitely and a user opening
 * the same id later would see ghost local edits.
 */
import { onBeforeUnmount, onMounted } from 'vue'

import { useSSE } from '@/services/sseService'
import { unwrapEventData, type TicketDeletedEventData } from '@/types/sse'
import { useCollabSessionStore } from '@/stores/collabSession'
import { useTicketDraftsStore } from '@/stores/ticketDrafts'
import { useTicketUiStore } from '@/stores/ticketUi'

export function useTicketDeletionCleanup(): void {
  const sse = useSSE()
  const collab = useCollabSessionStore()
  const drafts = useTicketDraftsStore()
  const ui = useTicketUiStore()

  const handler = (raw: unknown) => {
    const data = unwrapEventData(raw as TicketDeletedEventData)
    const id = data?.ticket_id
    if (typeof id !== 'number') return

    // Fire-and-forget; purgeData awaits IDB but we don't gate
    // anything on it. Errors are logged inside the store.
    void collab.purgeData(`ticket-${id}`)
    drafts.clearDraft(id)
    ui.clearAttachments(id)
    ui.clearPluginActivations(id)
  }

  onMounted(() => {
    if (!sse.isConnected.value) sse.connect()
    sse.addEventListener('ticket-deleted', handler)
  })
  onBeforeUnmount(() => {
    sse.removeEventListener('ticket-deleted', handler)
  })
}
