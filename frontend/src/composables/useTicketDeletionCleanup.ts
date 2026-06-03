/**
 * Ticket-deletion cleanup.
 *
 * Server-authoritative SSE: when the server announces
 * `ticket-deleted`, wipe every local artefact for that ticket so
 * a future open or refresh doesn't surface stale data the user
 * shouldn't see again. Artefacts:
 *
 *   - Yjs collab session (if active in this tab) and its on-disk
 *     IndexedDB cache, via `useCollabSessionStore.purgeData`.
 *   - In-progress comment draft text/internal flag (localStorage)
 *     via `useTicketDraftsStore.clearDraft`.
 *   - In-memory pending attachments + plugin-activation counters
 *     via `useTicketUiStore.clear*`.
 *   - Recent-tickets sidebar / dashboard widget cache, so the
 *     deleted entry vanishes without a manual refresh. The
 *     backend's `ON DELETE CASCADE` on `user_ticket_views.ticket_id`
 *     plus the INNER JOIN in `get_recent_tickets` mean a refetch
 *     produces the correct list; we just need to trigger it.
 *   - The current route, if the user is on the deleted ticket's
 *     detail page. Without this, the page is left half-rendered
 *     with stale chrome and every subsequent fetch surfaces a
 *     404-shaped error.
 *
 * Registered once at app boot from `App.vue` so the listener
 * lives for the entire session, regardless of which view is
 * currently mounted.
 */
import { onBeforeUnmount, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useQueryCache } from '@pinia/colada'

import { useSSE } from '@/services/sseService'
import { unwrapEventData, type TicketDeletedEventData } from '@/types/sse'
import { useCollabSessionStore } from '@/stores/collabSession'
import { useTicketDraftsStore } from '@/stores/ticketDrafts'
import { useTicketUiStore } from '@/stores/ticketUi'
import { useMyWorkspacesStore } from '@/stores/myWorkspaces'
import { RECENT_TICKETS_KEY } from '@/stores/recentTickets'
import { ticketDetailKey } from '@/loaders/ticketDetailLoader'
import { buildCollabDocId } from '@/utils/collabDocId'

export function useTicketDeletionCleanup(): void {
  const sse = useSSE()
  const collab = useCollabSessionStore()
  const drafts = useTicketDraftsStore()
  const ui = useTicketUiStore()
  const queryCache = useQueryCache()
  const router = useRouter()
  const route = useRoute()
  const workspaces = useMyWorkspacesStore()

  const handler = (raw: unknown) => {
    const data = unwrapEventData(raw as TicketDeletedEventData)
    const id = data?.ticket_id
    if (typeof id !== 'number') return

    // Fire-and-forget; purgeData awaits IDB but we don't gate
    // anything on it. Errors are logged inside the store. The
    // docId is workspace-namespaced (see utils/collabDocId.ts)
    // so the purge targets the same IDB key the live editor
    // would have constructed — without the prefix this wipe would
    // miss the cached doc entirely.
    const uuid = workspaces.activeWorkspace?.workspace_uuid
    if (uuid) {
      void collab.purgeData(buildCollabDocId(uuid, 'ticket', id))
    }
    drafts.clearDraft(id)
    ui.clearAttachments(id)
    ui.clearPluginActivations(id)

    // Recent-tickets sidebar / dashboard widget: drop the cached
    // detail row, then invalidate the list so Pinia Colada
    // refetches the server-authoritative view.
    queryCache.invalidateQueries({ key: ticketDetailKey(id), exact: true })
    queryCache.invalidateQueries({ key: RECENT_TICKETS_KEY })

    // If we're currently viewing the ticket that just got deleted,
    // navigate away before the next render tries to read from
    // half-purged state.
    if (route.name === 'ticket-view' && Number(route.params.id) === id) {
      void router.push('/tickets')
    }
  }

  onMounted(() => {
    if (!sse.isConnected.value) sse.connect()
    sse.addEventListener('ticket-deleted', handler)
  })
  onBeforeUnmount(() => {
    sse.removeEventListener('ticket-deleted', handler)
  })
}
