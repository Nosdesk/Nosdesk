/**
 * Assets list query layer. Owns the cache-key family the view
 * (`useInfiniteQuery` in `AssetsListView.vue`) subscribes to.
 */
import { listKeys } from './listKeys'

export const assetsKeys = listKeys('assets')
