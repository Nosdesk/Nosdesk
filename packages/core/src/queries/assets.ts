/**
 * Assets list query layer. Owns the cache-key family the view
 * (`useInfiniteQuery` in `AssetsListView.vue`) subscribes to.
 */
import { listKeys } from './listKeys'

export const assetsKeys = listKeys('assets')

/**
 * Cache key for a single asset's full detail record. Distinct from the
 * `assetsKeys.list(...)` family so the detail view (`useAssetDetail`) caches
 * and invalidates per-asset without touching list pages.
 */
export const assetDetailKey = (id: number | string) =>
  ['assets', 'detail', String(id)] as const
