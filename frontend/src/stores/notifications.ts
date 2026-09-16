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
 * Architecture decisions:
 *
 *   - Each inbox tab is its own server-filtered infinite query;
 *     pages live in the cache, concatenated for display.
 *   - Optimistic edits apply to every tab's cache at once and
 *     re-check tab membership, so a row marked read on the Unread
 *     tab leaves it without a refetch.
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
import { ref, toValue, type MaybeRefOrGetter } from 'vue'
import {
  setInfiniteQueryData,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryCache,
  type UseInfiniteQueryData,
} from '@pinia/colada'
import {
  archiveNotifications,
  deleteNotifications,
  getNotifications,
  getUnreadCount,
  getUnseenCount,
  markAllNotificationsRead,
  markAllSeen,
  markNotificationsRead,
  markNotificationsUnread,
  snoozeNotifications,
  type Notification,
} from '@nosdesk/core/services/notificationService'
import { onSyncActions } from '@nosdesk/core/sync/observers'
import { workspaceReady } from '@/services/activeWorkspace'

const PAGE_SIZE = 20

/** Inbox tab. Each maps to one server-side filter and one list cache. */
export type NotificationFilter = 'all' | 'unread' | 'mentions'
export const NOTIFICATION_FILTERS: readonly NotificationFilter[] = ['all', 'unread', 'mentions']

/** Keyset cursor: the last row's `created_at` and `id`. */
interface Cursor {
  before: string
  before_id: number
}

/** One cached page. `next` is fixed at fetch time from the raw page length,
 *  so optimistic removals never make a tab claim it is caught up while
 *  rows remain on the server. */
export interface NotificationPage {
  items: Notification[]
  next: Cursor | null
}

/** Whether a row belongs in a tab. The optimistic layer applies this after
 *  every transform so a row marked read leaves the Unread cache on its own. */
export function belongsToFilter(filter: NotificationFilter, n: Notification): boolean {
  switch (filter) {
    case 'unread':
      return !n.is_read
    case 'mentions':
      return n.notification_type === 'mentioned'
    default:
      return true
  }
}

function filterParams(filter: NotificationFilter) {
  switch (filter) {
    case 'unread':
      return { unread_only: true }
    case 'mentions':
      return { notification_type: 'mentioned' }
    default:
      return {}
  }
}

// Hierarchical query keys. Exported so the SSE invalidator and
// any future cross-cutting consumer subscribe to the same
// strings without typo drift. `listRoot` is the prefix every
// per-filter list shares; invalidation and cancellation target it.
export const NOTIFICATIONS_KEYS = {
  root: ['notifications'] as const,
  listRoot: () => [...NOTIFICATIONS_KEYS.root, 'list'] as const,
  list: (filter: NotificationFilter) => [...NOTIFICATIONS_KEYS.listRoot(), filter] as const,
  unreadCount: () => [...NOTIFICATIONS_KEYS.root, 'unreadCount'] as const,
  unseenCount: () => [...NOTIFICATIONS_KEYS.root, 'unseenCount'] as const,
}

// ---- Queries -------------------------------------------------

type ListData = UseInfiniteQueryData<NotificationPage, Cursor | null>

/**
 * Paginated notification feed for one tab. Pages append into one cache
 * entry keyed by `notifications.list.<filter>`, filtered server-side so
 * the tab is the truth rather than a sieve over the loaded window. Both
 * the bell and the inbox call this; only one network request fires per
 * page across all subscribers.
 *
 * A tab opened for the first time is seeded from the `all` cache
 * (client-filtered) as placeholder data, so switching tabs paints at
 * once and the server page replaces it.
 */
export function useNotificationsList(filter: MaybeRefOrGetter<NotificationFilter>) {
  const queryCache = useQueryCache()
  // Object form (not the options getter): that form drops
  // `placeholderData`, and a reactive `key` re-keys the query anyway.
  return useInfiniteQuery<NotificationPage, Error, Cursor | null>({
    key: () => NOTIFICATIONS_KEYS.list(toValue(filter)),
    initialPageParam: null,
    query: async ({ pageParam }) => {
      const items = await getNotifications({
        limit: PAGE_SIZE,
        ...filterParams(toValue(filter)),
        ...(pageParam ?? {}),
      })
      const last = items[items.length - 1]
      return {
        items,
        next:
          items.length === PAGE_SIZE && last
            ? { before: last.created_at, before_id: last.id }
            : null,
      }
    },
    getNextPageParam: (lastPage) => lastPage.next,
    placeholderData: () => {
      const f = toValue(filter)
      if (f === 'all') return undefined
      const all = queryCache.getQueryData<ListData>(NOTIFICATIONS_KEYS.list('all'))
      if (!all) return undefined
      const items = all.pages.flatMap((p) => p.items).filter((n) => belongsToFilter(f, n))
      return { pages: [{ items, next: null }], pageParams: [null] }
    },
    // Hold until a workspace is selected (see useUnreadCount).
    enabled: () => workspaceReady(),
  })
}

/** Unread count (how many items are not yet read). Drives the inbox
 *  "unread" affordances; refetched alongside the list on SSE arrivals. */
export function useUnreadCount() {
  return useQuery({
    key: NOTIFICATIONS_KEYS.unreadCount(),
    query: () => getUnreadCount(),
    // Hold until a workspace is selected so the always-mounted bell doesn't fire
    // header-less on first login (NoWorkspaceSelected).
    enabled: () => workspaceReady(),
  })
}

/** Unseen count for the bell badge. Per the redesign the badge counts
 *  UNSEEN (cleared when the panel/inbox opens), which is distinct from
 *  unread: glancing at the bell clears the badge without marking every
 *  item read. Cheap query, refetched alongside the list on SSE arrivals. */
export function useUnseenCount() {
  return useQuery({
    key: NOTIFICATIONS_KEYS.unseenCount(),
    query: () => getUnseenCount(),
    // Hold until a workspace is selected (see useUnreadCount).
    enabled: () => workspaceReady(),
  })
}

// ---- Mutations -----------------------------------------------

type QueryCache = ReturnType<typeof useQueryCache>

interface MutationContext {
  previousLists: Partial<Record<NotificationFilter, ListData | undefined>>
  previousCount: number | undefined
}

/** Snapshot every present list cache + the count for rollback. */
function snapshot(queryCache: QueryCache): MutationContext {
  const previousLists: MutationContext['previousLists'] = {}
  for (const f of NOTIFICATION_FILTERS) {
    previousLists[f] = queryCache.getQueryData<ListData>(NOTIFICATIONS_KEYS.list(f))
  }
  return {
    previousLists,
    previousCount: queryCache.getQueryData<number>(NOTIFICATIONS_KEYS.unreadCount()),
  }
}

function rollback(queryCache: QueryCache, ctx: MutationContext | undefined) {
  if (!ctx) return
  for (const f of NOTIFICATION_FILTERS) {
    const prev = ctx.previousLists[f]
    if (prev !== undefined) setInfiniteQueryData(queryCache, NOTIFICATIONS_KEYS.list(f), prev)
  }
  if (ctx.previousCount !== undefined) {
    queryCache.setQueryData(NOTIFICATIONS_KEYS.unreadCount(), ctx.previousCount)
  }
}

/** A row-level optimistic edit: return the new row, or `null` to remove it. */
export type RowTransform = (n: Notification) => Notification | null

/**
 * Pure core of the optimistic layer. Applies a row transform to every
 * present tab cache, then drops rows that no longer belong to that tab (a
 * row marked read leaves Unread). The unread-count delta is counted once
 * per row across caches: the same row lives in up to three caches with
 * identical read-state.
 */
export function transformPages(
  lists: Partial<Record<NotificationFilter, ListData | undefined>>,
  transform: RowTransform,
): { lists: Partial<Record<NotificationFilter, ListData>>; delta: number } {
  const seen = new Set<number>()
  let delta = 0
  const note = (before: Notification, after: Notification | null) => {
    if (seen.has(before.id)) return
    seen.add(before.id)
    const wasUnread = !before.is_read
    const isUnread = after !== null && !after.is_read
    if (wasUnread && !isUnread) delta -= 1
    if (!wasUnread && isUnread) delta += 1
  }
  const out: Partial<Record<NotificationFilter, ListData>> = {}
  for (const f of NOTIFICATION_FILTERS) {
    const old = lists[f]
    if (!old) continue
    out[f] = {
      ...old,
      pages: old.pages.map((page) => ({
        ...page,
        items: page.items.flatMap((n) => {
          const next = transform(n)
          note(n, next)
          return next && belongsToFilter(f, next) ? [next] : []
        }),
      })),
    }
  }
  return { lists: out, delta }
}

/** Apply a row transform to the live caches (see `transformPages`). */
function transformLists(queryCache: QueryCache, transform: RowTransform) {
  const current: Partial<Record<NotificationFilter, ListData | undefined>> = {}
  for (const f of NOTIFICATION_FILTERS) {
    current[f] = queryCache.getQueryData<ListData>(NOTIFICATIONS_KEYS.list(f))
  }
  const { lists, delta } = transformPages(current, transform)
  for (const f of NOTIFICATION_FILTERS) {
    const next = lists[f]
    if (next) setInfiniteQueryData(queryCache, NOTIFICATIONS_KEYS.list(f), next)
  }
  adjustUnread(queryCache, delta)
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

/** Optimistically drop rows from every cached list; the unread count
 *  follows. Shared by every mutation that removes rows from the active
 *  inbox: dismiss (delete), archive, and snooze. */
function removeFromLists(queryCache: QueryCache, ids: Iterable<number>) {
  const idSet = new Set(ids)
  if (idSet.size === 0) return
  transformLists(queryCache, (n) => (idSet.has(n.id) ? null : n))
}

/** Optimistically set `is_read` on the given rows in every cached list. */
function setRead(queryCache: QueryCache, ids: Iterable<number>, read: boolean) {
  const idSet = new Set(ids)
  if (idSet.size === 0) return
  transformLists(queryCache, (n) =>
    idSet.has(n.id) && n.is_read !== read ? { ...n, is_read: read } : n,
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
   *  `setRead`, `removeFromLists`, `transformLists`. */
  optimistic: (vars: TVars, queryCache: QueryCache) => void
}) {
  return function useListMutation() {
    const queryCache = useQueryCache()
    return useMutation<unknown, TVars, Error, MutationContext>({
      mutation: spec.mutate,
      onMutate: async (vars) => {
        await queryCache.cancelQueries({ key: NOTIFICATIONS_KEYS.listRoot() })
        const ctx = snapshot(queryCache)
        spec.optimistic(vars, queryCache)
        return ctx
      },
      // The Pinia Colada `ctx` type is the union of our MutationContext
      // and an internal Partial<MutationContext>; cast back since
      // onMutate above always returns the full shape.
      onError: (_err, _vars, ctx) => rollback(queryCache, ctx as MutationContext | undefined),
      onSettled: () => {
        // Reconcile the count only. The lists already reflect our
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
  optimistic: (id, queryCache) => setRead(queryCache, [id], true),
})

export const useDismissMutation = defineListMutation<number>({
  mutate: (id) => deleteNotifications([id]),
  optimistic: (id, queryCache) => removeFromLists(queryCache, [id]),
})

/** Mark everything read, or (with a type) only that type: the Mentions
 *  tab's button clears mentions server-wide, not just the loaded window. */
export const useMarkAllReadMutation = defineListMutation<string | undefined>({
  mutate: (notificationType) => markAllNotificationsRead(notificationType),
  optimistic: (notificationType, queryCache) => {
    transformLists(queryCache, (n) =>
      n.is_read || (notificationType && n.notification_type !== notificationType)
        ? n
        : { ...n, is_read: true },
    )
    if (!notificationType) queryCache.setQueryData<number>(NOTIFICATIONS_KEYS.unreadCount(), 0)
  },
})

export const useMarkManyReadMutation = defineListMutation<number[]>({
  mutate: (ids) => markNotificationsRead(ids),
  optimistic: (ids, queryCache) => setRead(queryCache, ids, true),
})

/** Mark all notifications seen: clears the bell badge when the panel or
 *  inbox opens, WITHOUT marking anything read (seen != read). Operates
 *  on the unseen count only, not the list read-state, so it doesn't use
 *  the list-mutation factory. Optimistically zeroes the count, then lets
 *  the server settle it. */
export function useMarkAllSeenMutation() {
  const queryCache = useQueryCache()
  return useMutation<unknown, void, Error, { previous: number | undefined }>({
    mutation: () => markAllSeen(),
    onMutate: () => {
      const previous = queryCache.getQueryData<number>(NOTIFICATIONS_KEYS.unseenCount())
      queryCache.setQueryData(NOTIFICATIONS_KEYS.unseenCount(), 0)
      return { previous }
    },
    onError: (_err, _vars, ctx) => {
      if (ctx?.previous !== undefined) {
        queryCache.setQueryData(NOTIFICATIONS_KEYS.unseenCount(), ctx.previous)
      }
    },
    onSettled: () => {
      queryCache.invalidateQueries({ key: NOTIFICATIONS_KEYS.unseenCount() })
    },
  })
}

/** Archive a notification: reversible triage that drops it from the
 *  active inbox (the server hides archived rows), replacing the
 *  destructive dismiss. Optimistically removes it from every list. */
export const useArchiveMutation = defineListMutation<number>({
  mutate: (id) => archiveNotifications([id]),
  optimistic: (id, queryCache) => removeFromLists(queryCache, [id]),
})

/** Mark a single notification unread (inverse of mark-read): flips it
 *  back into the unread set and bumps the count. The row cannot be
 *  slotted into the Unread cache in order, so that cache is refetched;
 *  nobody is looking at it while acting from another tab. */
export const useMarkUnreadMutation = defineListMutation<number>({
  mutate: (id) => markNotificationsUnread([id]),
  optimistic: (id, queryCache) => {
    setRead(queryCache, [id], false)
    queryCache.invalidateQueries({ key: NOTIFICATIONS_KEYS.list('unread') })
  },
})

/** Snooze a notification until `until` (ISO string): the server hides
 *  it from the active inbox until then, so, like archive, it drops out
 *  of every list optimistically. */
export const useSnoozeMutation = defineListMutation<{ id: number; until: string }>({
  mutate: ({ id, until }) => snoozeNotifications([id], until),
  optimistic: ({ id }, queryCache) => removeFromLists(queryCache, [id]),
})

export const useDeleteManyMutation = defineListMutation<number[]>({
  mutate: (ids) => deleteNotifications(ids),
  optimistic: (ids, queryCache) => removeFromLists(queryCache, ids),
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

  function revalidateNotifications() {
    queryCache.invalidateQueries({ key: NOTIFICATIONS_KEYS.unseenCount() })
    queryCache.invalidateQueries({ key: NOTIFICATIONS_KEYS.unreadCount() })
    queryCache.invalidateQueries({ key: NOTIFICATIONS_KEYS.listRoot() })
  }

  // Coalesce bursts: several notifications arriving together (or a
  // reconnect backfill) would otherwise fire N serial multi-page
  // refetches. A trailing debounce collapses them into one.
  let refetchTimer: ReturnType<typeof setTimeout> | null = null
  function scheduleRevalidate() {
    if (refetchTimer) return
    refetchTimer = setTimeout(() => {
      refetchTimer = null
      revalidateNotifications()
    }, 400)
  }

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
    // Those private-group actions aren't replayed by the sync engine's
    // backfill after an SSE reconnect gap (tab sleep, ~hourly token
    // reconnect), so the badge and inbox can silently under-count.
    // Self-heal by revalidating when the tab regains focus or the
    // network returns. The store is app-lifetime, so these listeners
    // never need removing.
    if (typeof window !== 'undefined') {
      window.addEventListener('online', revalidateNotifications)
      document.addEventListener('visibilitychange', () => {
        if (document.visibilityState === 'visible') revalidateNotifications()
      })
    }
  }

  function handleSyncActions(actions: { aggregate: string; data: unknown }[]) {
    try {
      const notes = actions.filter((a) => a.aggregate === 'notification')
      if (notes.length === 0) return
      // Refetch so new items slot in with correct ordering and metadata
      // (cheaper than prepending client-side and racing pagination),
      // debounced so a burst collapses into one revalidation.
      scheduleRevalidate()
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
