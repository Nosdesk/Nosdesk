/**
 * Vue-side glue for the workspace-namespaced collab docId helper.
 *
 * Reads the active workspace's UUID from the shared store and
 * exposes a builder that templates over the resource kind + the
 * resource's immutable UUID (never the recyclable integer id).
 * Components that hand a docId to `<CollaborativeEditor>` /
 * `useCollabSessionStore.acquire()` use this rather than
 * constructing the string themselves, so the namespace contract
 * is enforced in one place and the build helper's UUID-shape
 * assertion fires at the right altitude (composable boundary)
 * rather than deep inside the editor.
 *
 * Returns a `ComputedRef<string | null>` because the workspace
 * UUID is technically nullable while the caller's auth/me query
 * resolves; consumers should gate the editor on a non-null value
 * the same way they gate on any other auth-derived state.
 */
import { computed, type ComputedRef, type MaybeRefOrGetter, toValue } from 'vue'
import { useMyWorkspacesStore } from '@/stores/myWorkspaces'
import { buildCollabDocId, type CollabDocKind } from '@nosdesk/core/utils/collabDocId'

export function useCollabDocId(
  kind: CollabDocKind,
  resourceUuid: MaybeRefOrGetter<string | null | undefined>,
): ComputedRef<string | null> {
  const workspaces = useMyWorkspacesStore()
  return computed(() => {
    const uuid = workspaces.activeWorkspace?.workspace_uuid
    const resource = toValue(resourceUuid)
    if (!uuid || !resource) return null
    return buildCollabDocId(uuid, kind, resource)
  })
}
