/**
 * Projects sync facade.
 *
 * Thin wrapper over `useEntity` / `useAggregate` for the project
 * aggregate. Mutations (rename, status flip) go through
 * `dispatchOptimistic` so the UI updates instantly and the network
 * round-trip happens in the background.
 *
 * No reactive state of its own — Pinia DevTools sees the methods
 * and the absence of state. Pool inspection lives in a future
 * dev-only `__sync_debug` store, not here.
 */
import { defineStore } from 'pinia'
import { computed, toValue, type ComputedRef, type MaybeRefOrGetter } from 'vue'
import { useEntity, useAggregate } from '@/sync/composables'
import { dispatchOptimistic } from '@/sync/queue'

export interface SyncProject {
  id: number
  name: string
  description: string | null
  status: string
  created_at: string
  updated_at?: string
  created_by?: string | null
}

export const useSyncProjectsStore = defineStore('syncProjects', () => {
  function byId(id: MaybeRefOrGetter<number | null>): ComputedRef<SyncProject | null> {
    return useEntity<SyncProject>('project', () => toValue(id))
  }

  function all(): ComputedRef<SyncProject[]> {
    return useAggregate<SyncProject>('project')
  }

  /**
   * Optimistically rename a project. The pool flips to the new
   * name immediately; the server hears about it on the next push
   * tick. On rejection, the inverse patch reverts the pool.
   */
  async function rename(projectId: number, newName: string): Promise<void> {
    const current = useEntity<SyncProject>('project', projectId).value
    if (!current) return
    const previousName = current.name
    if (previousName === newName) return
    await dispatchOptimistic<SyncProject>('project', projectId, {
      forward: { name: newName, updated_at: new Date().toISOString() },
      inverse: { name: previousName, updated_at: current.updated_at },
    })
  }

  /**
   * Flip a project's status. Same optimistic shape as rename().
   */
  async function setStatus(projectId: number, status: string): Promise<void> {
    const current = useEntity<SyncProject>('project', projectId).value
    if (!current) return
    const previousStatus = current.status
    if (previousStatus === status) return
    await dispatchOptimistic<SyncProject>('project', projectId, {
      forward: { status, updated_at: new Date().toISOString() },
      inverse: { status: previousStatus, updated_at: current.updated_at },
    })
  }

  // Sorted-by-name view; most projects-list UIs want this. Computed
  // here once so consumers don't each re-sort on every render.
  const sortedByName = computed(() =>
    [...all().value].sort((a, b) => a.name.localeCompare(b.name)),
  )

  return { byId, all, sortedByName, rename, setStatus }
})
