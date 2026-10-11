/**
 * Documentation deletion cleanup.
 *
 * A documentation page or collection the reader loses reaches them as a
 * delete on the `sync_actions` stream: deleted for good, or no longer
 * theirs to open (the feed sends a bare delete for both, so the two read
 * the same). Either way its Yjs copy on this device goes, IndexedDB
 * included, whether or not it is open, so a later open or refresh can't
 * show it again. A view showing it is replaced with the not-found page, the
 * same one it shows when opened by link.
 *
 * A retraction names only the record's id, so the uuid in its collab doc id
 * comes from the row the pool held before the delete (`SyncAction.prior`).
 * A page moved to the trash is an update, not a delete, and a pool wipe on a
 * workspace switch or resync reaches no observer, so neither clears anything.
 *
 * Registered once at app boot from `App.vue`, next to the ticket cleanup.
 */
import { useRoute, useRouter } from 'vue-router'

import { useSyncActions } from '@/composables/useSyncActions'
import { useCollabSessionStore } from '@/stores/collabSession'
import { useMyWorkspacesStore } from '@/stores/myWorkspaces'
import { buildCollabDocId, type CollabDocKind } from '@nosdesk/core/utils/collabDocId'

interface LostRow {
  id?: unknown
  uuid?: unknown
  slug?: unknown
}

export function useDocDeletionCleanup(): void {
  const collab = useCollabSessionStore()
  const router = useRouter()
  const route = useRoute()
  const workspaces = useMyWorkspacesStore()

  const purge = (kind: CollabDocKind, row: LostRow) => {
    const workspaceUuid = workspaces.activeWorkspace?.workspace_uuid
    if (!workspaceUuid || typeof row.uuid !== 'string') return
    void collab.purgeData(buildCollabDocId(workspaceUuid, kind, row.uuid))
  }

  // A page's route carries its slug, or its id when it has none (docUrl).
  const isOpenPage = (row: LostRow) =>
    route.name === 'documentation-page' &&
    [String(row.id), String(row.slug)].includes(String(route.params.path))

  const isOpenCollection = (row: LostRow) =>
    route.name === 'collection-view' && String(route.params.slug) === String(row.slug)

  useSyncActions(
    (actions) => {
      let closeView = false
      for (const action of actions) {
        if (action.op !== 'D' || !action.prior) continue
        const row = action.prior as LostRow
        if (action.aggregate === 'documentation_page') {
          purge('doc', row)
          closeView ||= isOpenPage(row)
        } else {
          purge('collection', row)
          closeView ||= isOpenCollection(row)
        }
      }
      if (closeView) void router.replace({ name: 'error', params: { code: '404' } })
    },
    { aggregates: ['documentation_page', 'documentation_collection'] },
  )
}
