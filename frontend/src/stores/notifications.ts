/**
 * Notification data layer, built on Pinia Colada.
 *
 * Two surfaces consume these primitives: the header
 * `<NotificationBell>` (preview popover / bottom sheet) and
 * `<NotificationInboxView>` (full-page inbox). Both call the
 * same `useNotificationsList` and `useUnreadCount` composables;
 * Pinia Colada keys ensure they share one cache entry. Marking
 * an item read in one surface updates the other instantly.
 *
 * Architecture decisions (see
 * `~/Documents/notes/technology/web development/loading-states-architecture.md`):
 *
 *   - The list is an infinite query; pages live in the cache,
 *     concatenated for display via `data.pages.flat()`.
 *   - Mutations are optimistic with rollback context, then
 *     `invalidate` the unread count to let the server settle
 *     it (other devices may have changed it). The list is NOT
 *     invalidated on success because our optimistic update is
 *     authoritative for the operation we just performed.
 *   - SSE arrivals invalidate both queries so Colada refetches
 *     the latest server state. The Pinia store owns SSE wiring
 *     and the screen-reader announcement; everything else lives
 *     in Colada composables.
 */
import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  setInfiniteQueryData,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryCache,
  type UseInfiniteQueryData,
} from '@pinia/colada'
import {
  deleteNotifications,
  getNotifications,
  getUnreadCount,
  markAllNotificationsRead,
  markNotificationsRead,
  type Notification,
} from '@/services/notificationService'
import { onSyncActions } from '@/sync/observers'

const PAGE_SIZE = 20

// Hierarchical query keys. Exported so the SSE invalidator and
// any future cross-cutting consumer subscribe to the same
// strings without typo drift.
export const NOTIFICATIONS_KEYS = {
  root: ['notifications'] as const,
  list: () => [...NOTIFICATIONS_KEYS.root, 'list'] as const,
  unreadCount: () => [...NOTIFICATIONS_KEYS.root, 'unreadCount'] as const,
}

// ---- Queries -------------------------------------------------

/**
 * Paginated notification feed. Pages append into one cache entry
 * keyed by `notifications.list`. Both the bell and the inbox
 * call this; only one network request fires per page across all
 * subscribers.
 */
export function useNotificationsList() {
  return useInfiniteQuery({
    key: NOTIFICATIONS_KEYS.list(),
    initialPageParam: 0,
    query: ({ pageParam }) =>
      getNotifications({ limit: PAGE_SIZE, offset: pageParam }),
    // Next page param is the running total length, since the
    // backend uses offset-based pagination. `null` signals "no
    // more pages" (last page returned fewer than PAGE_SIZE items).
    getNextPageParam: (lastPage, allPages) =>
      lastPage.length === PAGE_SIZE
        ? allPages.flat().length
        : null,
  })
}

/** Unread count for the bell badge. Cheap query, refetched
 *  alongside the list on SSE arrivals. */
export function useUnreadCount() {
  return useQuery({
    key: NOTIFICATIONS_KEYS.unreadCount(),
    query: () => getUnreadCount(),
  })
}

// ---- Mutations -----------------------------------------------

type ListData = UseInfiniteQueryData<Notification[], number>
type QueryCache = ReturnType<typeof useQueryCache>

interface MutationContext {
  previousList: ListData | undefined
  previousCount: number | undefined
}

/** Snapshot the current list + count for rollback. */
function snapshot(queryCache: QueryCache): MutationContext {
  return {
    previousList: queryCache.getQueryData<ListData>(NOTIFICATIONS_KEYS.list()),
    previousCount: queryCache.getQueryData<number>(NOTIFICATIONS_KEYS.unreadCount()),
  }
}

function rollback(queryCache: QueryCache, ctx: MutationContext | undefined) {
  if (!ctx) return
  if (ctx.previousList !== undefined) {
    setInfiniteQueryData(queryCache, NOTIFICATIONS_KEYS.list(), ctx.previousList)
  }
  if (ctx.previousCount !== undefined) {
    queryCache.setQueryData(NOTIFICATIONS_KEYS.unreadCount(), ctx.previousCount)
  }
}

/** Apply a per-page transform to the infinite list cache.
 *  Centralises the page-mapping boilerplate every mutation needs. */
function transformList(
  queryCache: QueryCache,
  transform: (page: Notification[]) => Notification[],
) {
  setInfiniteQueryData<Notification[], Error, number>(
    queryCache,
    NOTIFICATIONS_KEYS.list(),
    (old) => {
      if (!old) return old as never
      return { ...old, pages: old.pages.map(transform) }
    },
  )
}

/** Adjust the cached unread count by `delta` (positive or
 *  negative), clamped at zero. */
function adjustUnread(queryCache: QueryCache, delta: number) {
  if (delta === 0) return
  queryCache.setQueryData<number>(
    NOTIFICATIONS_KEYS.unreadCount(),
    (old) => Math.max(0, (old ?? 0) + delta),
  )
}

/** Factory for "list-mutation" composables. Every notification
 *  mutation follows the same lifecycle: cancel pending fetches,
 *  snapshot for rollback, apply the optimistic update, reconcile
 *  the unread count on settle. The factory bottles that pattern
 *  so each individual mutation only describes what's unique to
 *  it (the API call + the optimistic transform). */
function defineListMutation<TVars>(spec: {
  mutate: (vars: TVars) => Promise<unknown>
  /** Apply the optimistic update synchronously. Use the helpers
   *  `transformList` and `adjustUnread` from the closure. */
  optimistic: (vars: TVars, queryCache: QueryCache) => void
}) {
  return function useListMutation() {
    const queryCache = useQueryCache()
    return useMutation<unknown, TVars, Error, MutationContext>({
      mutation: spec.mutate,
      onMutate: async (vars) => {
        await queryCache.cancelQueries({ key: NOTIFICATIONS_KEYS.list() })
        const ctx = snapshot(queryCache)
        spec.optimistic(vars, queryCache)
        return ctx
      },
      // The Pinia Colada `ctx` type is the union of our MutationContext
      // and an internal Partial<MutationContext>; cast back since
      // onMutate above always returns the full shape.
      onError: (_err, _vars, ctx) => rollback(queryCache, ctx as MutationContext | undefined),
      onSettled: () => {
        // Reconcile the count only. The list already reflects our
        // optimistic truth for the operation we performed; the
        // count may have moved due to other devices, so we let
        // the server settle it.
        queryCache.invalidateQueries({ key: NOTIFICATIONS_KEYS.unreadCount() })
      },
    })
  }
}

export const useMarkReadMutation = defineListMutation<number>({
  mutate: (id) => markNotificationsRead([id]),
  optimistic: (id, queryCache) => {
    let wasUnread = false
    transformList(queryCache, (page) =>
      page.map((n) => {
        if (n.id !== id || n.is_read) return n
        wasUnread = true
        return { ...n, is_read: true }
      }),
    )
    if (wasUnread) adjustUnread(queryCache, -1)
  },
})

export const useDismissMutation = defineListMutation<number>({
  mutate: (id) => deleteNotifications([id]),
  optimistic: (id, queryCache) => {
    let removedUnread = false
    transformList(queryCache, (page) =>
      page.filter((n) => {
        if (n.id !== id) return true
        if (!n.is_read) removedUnread = true
        return false
      }),
    )
    if (removedUnread) adjustUnread(queryCache, -1)
  },
})

export const useMarkAllReadMutation = defineListMutation<void>({
  mutate: () => markAllNotificationsRead(),
  optimistic: (_vars, queryCache) => {
    transformList(queryCache, (page) =>
      page.map((n) => (n.is_read ? n : { ...n, is_read: true })),
    )
    queryCache.setQueryData<number>(NOTIFICATIONS_KEYS.unreadCount(), 0)
  },
})

export const useMarkManyReadMutation = defineListMutation<number[]>({
  mutate: (ids) => markNotificationsRead(ids),
  optimistic: (ids, queryCache) => {
    if (ids.length === 0) return
    const idSet = new Set(ids)
    let flipped = 0
    transformList(queryCache, (page) =>
      page.map((n) => {
        if (!idSet.has(n.id) || n.is_read) return n
        flipped++
        return { ...n, is_read: true }
      }),
    )
    if (flipped > 0) adjustUnread(queryCache, -flipped)
  },
})

export const useDeleteManyMutation = defineListMutation<number[]>({
  mutate: (ids) => deleteNotifications(ids),
  optimistic: (ids, queryCache) => {
    if (ids.length === 0) return
    const idSet = new Set(ids)
    let removedUnread = 0
    transformList(queryCache, (page) =>
      page.filter((n) => {
        if (!idSet.has(n.id)) return true
        if (!n.is_read) removedUnread++
        return false
      }),
    )
    if (removedUnread > 0) adjustUnread(queryCache, -removedUnread)
  },
})

// ---- SSE wiring + screen-reader announcement ----------------

/**
 * Owns the SSE subscription and the screen-reader announcement.
 * Cache invalidation flows through here too: on a notification
 * arrival we tell Colada to refetch list + count, then the bell
 * and inbox update via their `useQuery` subscriptions.
 *
 * The `lastAnnouncement` ref backs the global polite live region
 * mounted in `<NotificationBell>`. The counter suffix forces
 * Vue to treat each set as a new mutation even when two
 * notifications share a title.
 */
/** Screen-reader announcement payload. Stored as structured data
 *  so the consumer (the bell component, which has Vue setup
 *  context) can format the announcement via fluent-vue. The
 *  store itself runs outside a setup scope and can't call
 *  `useFluent`, so it just records the title + sequence here. */
export interface NotificationAnnouncement {
  /** Notification title if the SSE payload carried one;
   *  `null` triggers the title-less variant in the catalogue. */
  title: string | null
  /** Monotonic counter, forces Vue to treat each set as a new
   *  mutation even when two notifications share a title. */
  seq: number
}

export const useNotificationsStore = defineStore('notifications', () => {
  const queryCache = useQueryCache()
  const lastAnnouncement = ref<NotificationAnnouncement | null>(null)
  let announcementSeq = 0
  let subscribed = false

  function ensureSubscribed() {
    if (subscribed) return
    subscribed = true
    // React to `notification` sync actions (cross-machine via Postgres
    // NOTIFY). The sync engine owns the connection; the emit is scoped
    // to the recipient's private `user:<uuid>` group, so this client
    // only ever receives its own notifications, no recipient filter
    // needed. The store lives for the app lifetime, so we don't retain
    // the unsubscribe handle.
    onSyncActions(handleSyncActions)
  }

  function handleSyncActions(actions: { aggregate: string; data: unknown }[]) {
    try {
      const notes = actions.filter((a) => a.aggregate === 'notification')
      if (notes.length === 0) return
      // Refetch so new items slot in with correct ordering and
      // metadata. Cheaper than prepending client-side and racing the
      // next pagination call.
      queryCache.invalidateQueries({ key: NOTIFICATIONS_KEYS.list() })
      queryCache.invalidateQueries({ key: NOTIFICATIONS_KEYS.unreadCount() })
      // Announce the newest arrival to screen-reader users.
      announcementSeq++
      const newest = notes[notes.length - 1].data as { title?: string } | undefined
      const title = newest?.title?.trim()
      lastAnnouncement.value = {
        title: title ? title : null,
        seq: announcementSeq,
      }
    } catch (error) {
      console.error('Error handling notification sync actions:', error)
    }
  }

  return { lastAnnouncement, ensureSubscribed }
})
