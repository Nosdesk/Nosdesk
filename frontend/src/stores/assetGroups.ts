import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { logger } from '@/utils/logger'
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

  async function load(force = false): Promise<AssetGroupSummary[]> {
    if (loaded.value && !force) return groups.value
    if (inflight) return inflight

    loading.value = true
    error.value = null

    inflight = (async () => {
      try {
        // The picker / facet never want archived groups.
        const next = await listAssetGroups(false)
        groups.value = next
        loaded.value = true
        return next
      } catch (e) {
        logger.error('Failed to load asset groups', e)
        error.value =
          e instanceof Error
            ? e.message
            : translate('error-store-asset-groups-load', undefined, 'Failed to load asset groups')
        return groups.value
      } finally {
        loading.value = false
        inflight = null
      }
    })()

    return inflight
  }

  function reset() {
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
