/**
 * Pinia Colada wrapper around the asset-kinds list. Consumers
 * (admin CRUD page, asset detail picker) get cache-first reads
 * and silent SWR revalidation; an admin write invalidating the
 * shared key refreshes every open view in the session.
 *
 * Returns the same composable surface the rest of the codebase
 * expects: `data`, `status`, `error`, plus a derived `isFirstLoad`
 * for the cold-start skeleton gate and a derived `kinds` ref that
 * coerces `data.value` to an empty array when undefined.
 */
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'

import {
  assetKindsService,
  ASSET_KINDS_QUERY_KEY,
  type AssetKind,
} from '@nosdesk/core/services/assetKindsService'

export function useAssetKindsQuery() {
  const query = useQuery({
    key: ASSET_KINDS_QUERY_KEY,
    query: () => assetKindsService.list(),
  })

  const kinds = computed<AssetKind[]>(() =>
    Array.isArray(query.data.value) ? query.data.value : [],
  )

  // Cold first load (no cached payload yet) is what should drive
  // a skeleton; subsequent navigations have cached data even while
  // a background revalidation is in flight.
  const isFirstLoad = computed(
    () => query.status.value === 'pending' && query.data.value === undefined,
  )

  return {
    kinds,
    data: query.data,
    status: query.status,
    error: query.error,
    isFirstLoad,
    refetch: query.refetch,
    asyncStatus: query.asyncStatus,
  }
}
