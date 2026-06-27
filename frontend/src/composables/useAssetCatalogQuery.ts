/**
 * Pinia Colada wrappers around the make/model catalog. Cache-first
 * reads with silent SWR; inline quick-create invalidates the shared
 * keys so every open picker refreshes.
 */
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'

import {
  manufacturersService,
  assetModelsService,
  MANUFACTURERS_QUERY_KEY,
  ASSET_MODELS_QUERY_KEY,
} from '@nosdesk/core/services/assetCatalogService'
import type { Manufacturer, AssetModel } from '@nosdesk/core/types/asset'

export function useManufacturersQuery() {
  const query = useQuery({
    key: MANUFACTURERS_QUERY_KEY,
    query: () => manufacturersService.list(),
  })
  const manufacturers = computed<Manufacturer[]>(() =>
    Array.isArray(query.data.value) ? query.data.value : [],
  )
  return { manufacturers, status: query.status, error: query.error, refetch: query.refetch }
}

export function useAssetModelsQuery() {
  const query = useQuery({
    key: ASSET_MODELS_QUERY_KEY,
    query: () => assetModelsService.list(),
  })
  const models = computed<AssetModel[]>(() =>
    Array.isArray(query.data.value) ? query.data.value : [],
  )
  return { models, status: query.status, error: query.error, refetch: query.refetch }
}
