import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { logger } from '@nosdesk/core/utils/logger'
import { translate } from '@/i18n'
import { listAssetGroups, type AssetGroupSummary } from '@/services/assetGroupService'

/**
 * Native asset groups are a small, slow-moving set, read on the asset list
 * (filter facet) and the asset detail (group picker). Mirrors the workflow-
 * states store: load once, keep in memory until signout or an admin write
 * invalidates it. Cache-first, no skeleton.
 */
export const useAssetGroupsStore = defineStore('assetGroups', () => {
  const groups = ref<AssetGroupSummary[]>([])
  const loaded = ref(false)
  const loading = ref(false)
  const error = ref<string | null>(null)

  let inflight: Promise<AssetGroupSummary[]> | null = null
  // Bumped by reset(): a load started before a workspace switch must not
  // write its result, nor clear the new workspace's in-flight load.
  let generation = 0

  async function load(force = false): Promise<AssetGroupSummary[]> {
    if (loaded.value && !force) return groups.value
    if (inflight) return inflight

    loading.value = true
    error.value = null

    const gen = generation
    const run: Promise<AssetGroupSummary[]> = (async () => {
      try {
        // The picker / facet never want archived groups.
        const next = await listAssetGroups(false)
        if (gen !== generation) return next
        groups.value = next
        loaded.value = true
        return next
      } catch (e) {
        if (gen !== generation) return groups.value
        logger.error('Failed to load asset groups', e)
        error.value =
          e instanceof Error
            ? e.message
            : translate('error-store-asset-groups-load', undefined, 'Failed to load asset groups')
        return groups.value
      } finally {
        if (gen === generation) {
          inflight = null
          loading.value = false
        }
      }
    })()
    inflight = run

    return run
  }

  function reset() {
    generation++
    inflight = null
    loading.value = false
    groups.value = []
    loaded.value = false
    error.value = null
  }

  function findById(id: number): AssetGroupSummary | undefined {
    return groups.value.find((g) => g.id === id)
  }

  /** Active (non-archived) groups, ordered as the API returned them. */
  const active = computed<AssetGroupSummary[]>(() =>
    groups.value.filter((g) => !g.archived_at),
  )

  return { groups, loaded, loading, error, active, load, reset, findById }
})
