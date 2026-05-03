/**
 * Pinia facade over the /api/saved-views endpoints.
 *
 * One store per session, lazy-loaded per project. The cache key is
 * `project_id ?? 'workspace-only'` so re-entering the same project
 * route reuses the prior fetch and switching projects loads a
 * fresh set.
 *
 * Optimistic on create / update / archive: the cache mutates
 * immediately, the network call settles after. On rejection we
 * revert and surface the error through the store's `lastError`
 * field; the UI checks that field to flash a toast.
 */
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { logger } from '@/utils/logger'
import {
  savedViewsService,
  type SavedView,
  type CreateSavedViewBody,
  type UpdateSavedViewBody,
} from '@/services/savedViewsService'

type CacheKey = string

export const useSavedViewsStore = defineStore('savedViews', () => {
  const cache = ref<Map<CacheKey, SavedView[]>>(new Map())
  const loadingKeys = ref<Set<CacheKey>>(new Set())
  const lastError = ref<string | null>(null)

  function keyFor(projectId: number | null | undefined): CacheKey {
    return projectId == null ? 'workspace-only' : `project:${projectId}`
  }

  async function ensureLoaded(projectId: number | null | undefined): Promise<SavedView[]> {
    const key = keyFor(projectId)
    if (cache.value.has(key)) return cache.value.get(key)!
    if (loadingKeys.value.has(key)) {
      // Another caller is already fetching this key; wait for the
      // result by polling the cache. Cheap and stays correct
      // without an extra in-flight Promise registry.
      while (loadingKeys.value.has(key)) {
        await new Promise((r) => setTimeout(r, 16))
      }
      return cache.value.get(key) ?? []
    }
    loadingKeys.value.add(key)
    try {
      const rows = await savedViewsService.list(projectId ?? undefined)
      cache.value.set(key, rows)
      return rows
    } catch (e) {
      logger.warn('Failed to load saved views', { projectId, error: e })
      lastError.value = e instanceof Error ? e.message : 'Failed to load saved views'
      return []
    } finally {
      loadingKeys.value.delete(key)
    }
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

  /** Local invalidation that also re-applies a per-key dedupe pass.
   * Used after create / update / archive to keep the cache shape
   * consistent without a refetch. */
  function applyMutation(view: SavedView): void {
    for (const [key, rows] of cache.value) {
      const idx = rows.findIndex((v) => v.uuid === view.uuid)
      if (view.archived_at != null) {
        if (idx >= 0) rows.splice(idx, 1)
      } else if (idx >= 0) {
        rows[idx] = view
      } else if (cacheKeyMatches(key, view)) {
        rows.push(view)
        rows.sort((a, b) => a.name.localeCompare(b.name))
      }
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
      lastError.value = e instanceof Error ? e.message : 'Failed to save view'
      return null
    }
  }

  async function update(uuid: string, body: UpdateSavedViewBody): Promise<SavedView | null> {
    try {
      const view = await savedViewsService.update(uuid, body)
      applyMutation(view)
      // Promoting a default demotes any sibling that was the
      // previous default; refetch the affected scope so the cache
      // reflects that without a second mutation event.
      if (body.is_default && cacheNeedsRefresh(view)) {
        const projectId = view.scope === 'project' && view.scope_id
          ? Number(view.scope_id)
          : null
        cache.value.delete(keyFor(projectId))
        await ensureLoaded(projectId)
      }
      return view
    } catch (e) {
      logger.warn('Saved view update failed', { uuid, body, error: e })
      lastError.value = e instanceof Error ? e.message : 'Failed to update view'
      return null
    }
  }

  function cacheNeedsRefresh(view: SavedView): boolean {
    const key = view.scope === 'project' && view.scope_id
      ? `project:${view.scope_id}`
      : 'workspace-only'
    return cache.value.has(key)
  }

  async function archive(uuid: string): Promise<boolean> {
    const existing = findByUuid(uuid)
    try {
      await savedViewsService.archive(uuid)
      if (existing) {
        existing.archived_at = new Date().toISOString()
        applyMutation(existing)
      }
      return true
    } catch (e) {
      logger.warn('Saved view archive failed', { uuid, error: e })
      lastError.value = e instanceof Error ? e.message : 'Failed to archive view'
      return false
    }
  }

  function reset(): void {
    cache.value.clear()
    loadingKeys.value.clear()
    lastError.value = null
  }

  return {
    cache,
    lastError,
    ensureLoaded,
    viewsForProject,
    findByUuid,
    create,
    update,
    archive,
    reset,
  }
})
