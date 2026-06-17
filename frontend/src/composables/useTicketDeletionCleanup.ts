/**
 * Ticket-deletion cleanup.
 *
 * Server-authoritative: when a ticket delete lands on the
 * `sync_actions` change-stream (aggregate `ticket`, op `D`), wipe
 * every local artefact for that ticket so a future open or refresh
 * doesn't surface stale data the user shouldn't see again. Driving
 * this off the sync stream (rather than the old discrete
 * `ticket-deleted` event) makes it correct across backend machines,
 * since the stream is delivered everywhere via Postgres NOTIFY.
 * Artefacts:
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
import { useRoute, useRouter } from 'vue-router'
import { useQueryCache } from '@pinia/colada'
import { effectiveRouteName } from '@/router/workspaceRouting'

import { useSyncActions } from '@/composables/useSyncActions'
import { useCollabSessionStore } from '@/stores/collabSession'
import { useTicketDraftsStore } from '@/stores/ticketDrafts'
import { useTicketUiStore } from '@/stores/ticketUi'
import { useMyWorkspacesStore } from '@/stores/myWorkspaces'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { RECENT_TICKETS_KEY } from '@/stores/recentTickets'
import { ticketDetailKey } from '@/loaders/ticketDetailLoader'
import { buildCollabDocId } from '@/utils/collabDocId'

export function useTicketDeletionCleanup(): void {
  const collab = useCollabSessionStore()
  const drafts = useTicketDraftsStore()
  const ui = useTicketUiStore()
  const queryCache = useQueryCache()
  const router = useRouter()
  const route = useRoute()
  const workspaces = useMyWorkspacesStore()
  const ticketsStore = useSyncTicketsStore()

  const cleanupTicket = (id: number) => {
    // Fire-and-forget; purgeData awaits IDB but we don't gate anything
    // on it. The docId is keyed by the ticket's immutable UUID (see
    // utils/collabDocId.ts), so we resolve it from the pool. Best-effort:
    // if the pool row is already gone the orphaned cache is harmless (its
    // UUID never recycles) and the LRU prune reclaims it.
    const uuid = workspaces.activeWorkspace?.workspace_uuid
    const ticketUuid = ticketsStore.byId(id).value?.uuid
    if (uuid && ticketUuid) {
      void collab.purgeData(buildCollabDocId(uuid, 'ticket', ticketUuid))
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
    if (effectiveRouteName(route) === 'ticket-view' && Number(route.params.id) === id) {
      void router.push('/tickets')
    }
  }

  // React to ticket hard-deletes on the sync stream (aggregate
  // 'ticket', op 'D'). A single batch can carry several deletes; clean
  // up each. Ticket-aggregate rows carry the numeric id as aggregate_id.
  useSyncActions(
    (actions) => {
      for (const action of actions) {
        if (action.op !== 'D') continue
        const id = Number(action.aggregate_id)
        if (Number.isFinite(id)) cleanupTicket(id)
      }
    },
    { aggregates: ['ticket'] },
  )
}
