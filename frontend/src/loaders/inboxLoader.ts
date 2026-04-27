/**
 * Vue Router Data Loader for the /inbox route.
 *
 * Runs DURING navigation, before the inbox component code is
 * even executed. By the time the view mounts, the first page
 * of notifications and the unread count are already in Pinia
 * Colada's cache; the view's `useNotificationsList()` /
 * `useUnreadCount()` calls return the primed data instantly.
 *
 * This is the "render-as-you-fetch" pattern. The pay-off is
 * visible on slow networks: the network request and the route
 * chunk download happen in parallel rather than serially after
 * mount.
 *
 * Pattern for new routes:
 *   1. Create a loader file (`src/loaders/<route>Loader.ts`)
 *   2. `defineColadaLoader` with the underlying queries
 *   3. Use `setInfiniteQueryData` / `setQueryData` to prime the
 *      caches the route's components will consume
 *   4. Reference via `meta.loaders` in the router or export
 *      from the component file for auto-detection
 *
 * See `~/Documents/notes/technology/web development/loading-states-architecture.md`
 * for the architectural rationale.
 */
import { defineColadaLoader } from 'unplugin-vue-router/data-loaders/pinia-colada'
import { setInfiniteQueryData, useQueryCache } from '@pinia/colada'
import {
  getNotifications,
  getUnreadCount,
} from '@/services/notificationService'
import { NOTIFICATIONS_KEYS } from '@/stores/notifications'

const PAGE_SIZE = 20

export const useInboxLoader = defineColadaLoader({
  // The loader's own cache key. Distinct from the underlying
  // useInfiniteQuery / useUnreadCount keys; we use this entry
  // purely as the navigation lifecycle anchor and side-effect
  // into the real caches inside `query`.
  key: () => ['notifications', 'inbox-loader'],

  async query() {
    const queryCache = useQueryCache()

    // Fetch first page + unread count in parallel. Both are
    // server round-trips; doing them concurrently saves the
    // sequential cost. Aborting the request on cancelled
    // navigation would require threading an `AbortSignal`
    // through `notificationService`; left as a follow-up since
    // these requests are cheap.
    const [firstPage, unreadCount] = await Promise.all([
      getNotifications({ limit: PAGE_SIZE, offset: 0 }),
      getUnreadCount(),
    ])

    // Prime the infinite query cache with the first page.
    // useNotificationsList() will see this and render
    // immediately without firing its own request.
    setInfiniteQueryData(
      queryCache,
      NOTIFICATIONS_KEYS.list(),
      {
        pages: [firstPage],
        pageParams: [0],
      },
    )

    // Prime the unread count cache for the bell badge.
    queryCache.setQueryData(NOTIFICATIONS_KEYS.unreadCount(), unreadCount)

    return { firstPage, unreadCount }
  },
})
