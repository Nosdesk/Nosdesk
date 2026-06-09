/**
 * Pinia Colada wrapper around the distinct asset-locations list.
 * Consumers (asset detail suggestions, list-view location facet)
 * get cache-first reads and silent SWR revalidation off one shared
 * key, so the two views no longer each fire their own fetch.
 *
 * Mirrors `useAssetKindsQuery`: returns `data`, `status`, `error`,
 * a derived `locations` ref coerced to an array, and `isFirstLoad`
 * for any cold-start gate.
 */
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'

import {
  getAssetLocations,
  ASSET_LOCATIONS_QUERY_KEY,
  type AssetLocationOption,
} from '@/services/assetService'

export function useAssetLocationsQuery() {
  const query = useQuery({
    key: ASSET_LOCATIONS_QUERY_KEY,
    query: () => getAssetLocations(),
  })

  const locations = computed<AssetLocationOption[]>(() =>
    Array.isArray(query.data.value) ? query.data.value : [],
  )

  const isFirstLoad = computed(
    () => query.status.value === 'pending' && query.data.value === undefined,
  )

  return {
    locations,
    data: query.data,
    status: query.status,
    error: query.error,
    isFirstLoad,
    refetch: query.refetch,
    asyncStatus: query.asyncStatus,
  }
}
