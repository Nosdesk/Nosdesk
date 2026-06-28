import { ref, watch, type Ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useMyWorkspacesStore } from '@/stores/myWorkspaces'
import { subscribe } from '@/sync/lifecycle'

/**
 * Subscribe the sync pool to the ACTIVE workspace's group, re-subscribing when
 * the active workspace changes. A workspace switch may reuse the same component
 * instance (the route slug changes but the view stays mounted), so this watches
 * the id rather than firing once in `onMounted`. Replaces the hardcoded
 * `subscribe('workspace:1')` the list views used to call.
 *
 * Returns a `ready` ref that releases once the subscribe for the current
 * workspace settles, resolved OR errored (via `finally`), so a loading gate
 * never sticks on a bootstrap failure. `afterSubscribe` runs per workspace,
 * after its bootstrap, for workspace-scoped follow-up loads (e.g. saved views).
 */
export function useWorkspaceGroupSubscription(
  afterSubscribe?: (workspaceId: number) => unknown,
): { ready: Ref<boolean> } {
  const { activeWorkspaceId } = storeToRefs(useMyWorkspacesStore())
  const ready = ref(false)

  watch(
    activeWorkspaceId,
    async (id) => {
      if (id == null) return
      ready.value = false
      try {
        await subscribe(`workspace:${id}`)
        if (afterSubscribe) await afterSubscribe(id)
      } finally {
        ready.value = true
      }
    },
    { immediate: true },
  )

  return { ready }
}
