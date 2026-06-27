/**
 * Pinia facade over the /api/saved-views endpoints.
 *
 * One store per session, lazy-loaded per project. The cache key is
 * `project_id ?? 'workspace-only'` so re-entering the same project
 * route reuses the prior fetch and switching projects loads a
 * fresh set.
 *
 * Optimistic on create / update / delete: the cache mutates
 * immediately, the network call settles after. On rejection we
 * revert and surface the error through the store's `lastError`
 * field; the UI checks that field to flash a toast.
 */
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { logger } from '@nosdesk/core/utils/logger'
import { translate } from '@/i18n'
import { dedupeInFlight } from '@nosdesk/core/utils/dedupeInFlight'
import {
  savedViewsService,
  type SavedView,
  type CreateSavedViewBody,
  type UpdateSavedViewBody,
} from '@/services/savedViewsService'

type CacheKey = string

export const useSavedViewsStore = defineStore('savedViews', () => {
  const cache = ref<Map<CacheKey, SavedView[]>>(new Map())
  // In-flight fetch registry. Concurrent callers for the same key
  // share one Promise rather than polling each other to settle.
  const inflight = new Map<CacheKey, Promise<SavedView[]>>()
  const lastError = ref<string | null>(null)

  function keyFor(projectId: number | null | undefined): CacheKey {
    return projectId == null ? 'workspace-only' : `project:${projectId}`
  }

  async function ensureLoaded(projectId: number | null | undefined): Promise<SavedView[]> {
    const key = keyFor(projectId)
    const cached = cache.value.get(key)
    if (cached) return cached
    return dedupeInFlight(inflight, key, async () => {
      try {
        const rows = await savedViewsService.list(projectId ?? undefined)
        cache.value.set(key, rows)
        return rows
      } catch (e) {
        logger.warn('Failed to load saved views', { projectId, error: e })
        lastError.value =
          e instanceof Error
            ? e.message
            : translate('error-store-saved-views-load', undefined, 'Failed to load saved views')
        return []
      }
    })
  }

  /** Loader-side cache prime. Used by route loaders so the view's
   * first render sees the saved-view list synchronously instead
   * of racing against a background fetch. */
  function prime(projectId: number | null | undefined, rows: SavedView[]): void {
    cache.value.set(keyFor(projectId), rows)
  }

  function viewsForProject(projectId: number | null | undefined) {
    const key = keyFor(projectId)
    return computed(() => cache.value.get(key) ?? [])
  }

  function findByUuid(uuid: string): SavedView | undefined {
    for (const rows of cache.value.values()) {
      const hit = rows.find((v) => v.uuid === uuid)
      if (hit) return hit
    }
    return undefined
  }

  /** Local invalidation. Called after create / update so the cache
   * shape stays consistent without a refetch. Delete uses
   * `removeFromCache` since there's no row to merge in. */
  function applyMutation(view: SavedView): void {
    for (const [key, rows] of cache.value) {
      const idx = rows.findIndex((v) => v.uuid === view.uuid)
      if (idx >= 0) {
        rows[idx] = view
      } else if (cacheKeyMatches(key, view)) {
        rows.push(view)
        rows.sort((a, b) => a.name.localeCompare(b.name))
      }
    }
  }

  function removeFromCache(uuid: string): void {
    for (const rows of cache.value.values()) {
      const idx = rows.findIndex((v) => v.uuid === uuid)
      if (idx >= 0) rows.splice(idx, 1)
    }
  }

  function cacheKeyMatches(key: CacheKey, view: SavedView): boolean {
    if (view.scope === 'workspace' || view.scope === 'private') return true
    if (view.scope === 'project') {
      return key === `project:${view.scope_id}`
    }
    return false
  }

  async function create(body: CreateSavedViewBody): Promise<SavedView | null> {
    try {
      const view = await savedViewsService.create(body)
      applyMutation(view)
      return view
    } catch (e) {
      logger.warn('Saved view create failed', { body, error: e })
      lastError.value =
        e instanceof Error
          ? e.message
          : translate('error-store-saved-view-save', undefined, 'Failed to save view')
      return null
    }
  }

  async function update(uuid: string, body: UpdateSavedViewBody): Promise<SavedView | null> {
    try {
      const view = await savedViewsService.update(uuid, body)
      applyMutation(view)
      return view
    } catch (e) {
      logger.warn('Saved view update failed', { uuid, body, error: e })
      lastError.value =
        e instanceof Error
          ? e.message
          : translate('error-store-saved-view-update', undefined, 'Failed to update view')
      return null
    }
  }

  /** Hard delete. Removes the view's row from the DB; no
   * un-delete affordance — recreate the view if needed. */
  async function deleteView(uuid: string): Promise<boolean> {
    try {
      await savedViewsService.delete(uuid)
      removeFromCache(uuid)
      return true
    } catch (e) {
      logger.warn('Saved view delete failed', { uuid, error: e })
      lastError.value =
        e instanceof Error
          ? e.message
          : translate('error-store-saved-view-delete', undefined, 'Failed to delete view')
      return false
    }
  }

  function reset(): void {
    cache.value.clear()
    inflight.clear()
    lastError.value = null
  }

  return {
    cache,
    lastError,
    ensureLoaded,
    prime,
    viewsForProject,
    findByUuid,
    create,
    update,
    deleteView,
    reset,
  }
})
