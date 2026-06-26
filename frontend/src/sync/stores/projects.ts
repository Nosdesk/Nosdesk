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
import { upsert, remove as poolRemove, patch as poolPatch } from '@/sync/pool'
import { projectService } from '@/services/projectService'
import type { Project } from '@nosdesk/core/types/project'

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
      forward: { name: newName },
      inverse: { name: previousName },
    })
    // Local-only freshness hint for the Updated column; the server
    // sets `updated_at` on apply and SSE reconciles the canonical value.
    poolPatch('project', projectId, { updated_at: new Date().toISOString() })
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
      forward: { status },
      inverse: { status: previousStatus },
    })
    poolPatch('project', projectId, { updated_at: new Date().toISOString() })
  }

  /**
   * Seed the pool from a REST create response. The backend also emits
   * `project.created` over SSE, but that can lag; without this the
   * projects list stays stale until the event lands.
   */
  function ingestCreated(project: Project): void {
    upsert<SyncProject>('project', project.id, {
      id: project.id,
      name: project.name,
      description: project.description ?? null,
      status: project.status,
      created_at: project.created_at,
      updated_at: project.updated_at,
    })
  }

  /**
   * Remove a project from the pool immediately, then confirm via REST.
   * Restores the row if the server rejects the delete.
   */
  async function remove(projectId: number): Promise<void> {
    const current = useEntity<SyncProject>('project', projectId).value
    const snapshot = current
      ? {
          id: current.id,
          name: current.name,
          description: current.description,
          status: current.status,
          created_at: current.created_at,
          updated_at: current.updated_at,
          created_by: current.created_by,
        }
      : null

    poolRemove('project', projectId)

    try {
      await projectService.deleteProject(projectId)
    } catch (e) {
      if (snapshot) {
        upsert<SyncProject>('project', projectId, snapshot)
      }
      throw e
    }
  }

  // Sorted-by-name view; most projects-list UIs want this. Computed
  // here once so consumers don't each re-sort on every render.
  const sortedByName = computed(() =>
    [...all().value].sort((a, b) => a.name.localeCompare(b.name)),
  )

  return { byId, all, sortedByName, rename, setStatus, ingestCreated, remove }
})
