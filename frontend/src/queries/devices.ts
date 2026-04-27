/**
 * Devices list query layer. Owns the cache-key family the view
 * (`useInfiniteQuery` in `DevicesListView.vue`) subscribes to.
 */
import { listKeys } from './listKeys'

export const devicesKeys = listKeys('devices')
