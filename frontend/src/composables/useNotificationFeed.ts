/**
 * Shared wiring for notification surfaces (bell preview +
 * full-page inbox). Lifts the queries, mutations, derived state,
 * and presentation helpers both consumers need.
 *
 * Each surface still owns its own template, layout, empty-state
 * copy, date-grouping granularity, and any surface-specific
 * concerns (e.g., bulk selection in the inbox). What they SHARE
 * is what's collected here.
 */
import { computed, type ComputedRef } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  useDismissMutation,
  useMarkAllReadMutation,
  useMarkManyReadMutation,
  useMarkReadMutation,
  useNotificationsList,
  useUnreadCount,
} from '@/stores/notifications'
import type { Notification } from '@nosdesk/core/services/notificationService'
import type { IconName } from '@/components/common/icons'
import type { AsyncBoundaryOp } from '@/components/common/AsyncBoundary.vue'

export type NotificationFilter = 'all' | 'unread' | 'mentions'

export interface NotificationFilterTab {
  value: NotificationFilter
  label: string
}

/** Filter-tab descriptors with locale-aware labels. Exposed as a
 *  composable (rather than a module-level const) so the labels
 *  re-evaluate when the active locale changes. Must be called
 *  from a Vue setup context. */
export function useNotificationFilterTabs(): ComputedRef<
  ReadonlyArray<NotificationFilterTab>
> {
  const fluent = useFluent()
  return computed(() => [
    { value: 'all', label: fluent.$t('notifications-filter-tabs-all') },
    { value: 'unread', label: fluent.$t('notifications-filter-tabs-unread') },
    { value: 'mentions', label: fluent.$t('notifications-filter-tabs-mentions') },
  ])
}

const TYPE_ICON: Record<string, IconName> = {
  ticket_assigned: 'userPlus',
  ticket_status_changed: 'refresh',
  ticket_created_requester: 'add',
  comment_added: 'comment',
  mentioned: 'at',
  doc_page_updated: 'documentEdit',
  // The time-sensitive operational types carry their own glyph so the
  // eye pre-sorts them from the routine ticket/comment stream.
  asset_low_stock: 'consumable',
  sla_breached: 'warning',
  loan_due_soon: 'clock',
  loan_overdue: 'warning',
}

/** Map a notification type code to its display icon. Falls back
 *  to the generic bell so unknown / future types still render. */
export function iconForNotificationType(type: string): IconName {
  return TYPE_ICON[type] ?? 'bell'
}

/** Filter the loaded set by tab. Pure derivation so tab
 *  switches are instant (no refetch). */
export function applyNotificationFilter(
  filter: NotificationFilter,
  items: readonly Notification[],
): Notification[] {
  switch (filter) {
    case 'unread':
      return items.filter((n) => !n.is_read)
    case 'mentions':
      return items.filter((n) => n.notification_type === 'mentioned')
    default:
      return [...items]
  }
}

export interface NotificationFeed {
  /** Raw infinite-query handle, exposed so consumers can call
   *  `loadNextPage()` / `refresh()` directly. */
  list: ReturnType<typeof useNotificationsList>
  /** Unread-count handle, exposed for `refresh()` access. */
  unread: ReturnType<typeof useUnreadCount>

  // Derived display state ----------------------------------
  items: ComputedRef<Notification[]>
  unreadCount: ComputedRef<number>
  hasMore: ComputedRef<boolean>
  /** Op shape for `<AsyncBoundary>`. Projects Pinia Colada's
   *  status surface into the boundary's contract. */
  fetchOp: ComputedRef<AsyncBoundaryOp>
  /** Pending AND no items yet. Drives the first-load skeleton. */
  isFirstLoad: ComputedRef<boolean>
  /** Pending AND items already shown. Drives "load more" button
   *  state and any background-refresh indicator. */
  isLoadingMore: ComputedRef<boolean>

  // Mutations ----------------------------------------------
  markRead: ReturnType<typeof useMarkReadMutation>
  dismiss: ReturnType<typeof useDismissMutation>
  markAllRead: ReturnType<typeof useMarkAllReadMutation>
  markManyRead: ReturnType<typeof useMarkManyReadMutation>

  // Convenience handlers -----------------------------------
  /** Mark-all-read scoped to the active filter. "All" and "Unread"
   *  both use the global server endpoint (marking everything read is
   *  what either button means), so unread items beyond the loaded
   *  window are cleared and the badge actually zeroes. "Mentions"
   *  passes the visible unread ids so it doesn't clear unrelated
   *  notifications (no type-scoped bulk endpoint exists yet). */
  markAllReadScoped: (
    filter: NotificationFilter,
    visible: readonly Notification[],
  ) => void
}

export function useNotificationFeed(): NotificationFeed {
  const list = useNotificationsList()
  const unread = useUnreadCount()

  const markRead = useMarkReadMutation()
  const dismiss = useDismissMutation()
  const markAllRead = useMarkAllReadMutation()
  const markManyRead = useMarkManyReadMutation()

  const items = computed<Notification[]>(
    () => list.data.value?.pages.flat() ?? [],
  )
  const unreadCount = computed(() => unread.data.value ?? 0)
  const hasMore = computed(() => list.hasNextPage.value)

  const fetchOp = computed<AsyncBoundaryOp>(() => ({
    isPending: list.asyncStatus.value === 'loading',
    isError: list.status.value === 'error',
    error: list.error.value,
  }))
  const isFirstLoad = computed(
    () => fetchOp.value.isPending && items.value.length === 0,
  )
  const isLoadingMore = computed(
    () => fetchOp.value.isPending && items.value.length > 0,
  )

  function markAllReadScoped(
    filter: NotificationFilter,
    visible: readonly Notification[],
  ) {
    // 'unread' is semantically identical to 'all' (mark everything
    // read), so route it through the global endpoint too — otherwise
    // unread items beyond the loaded window stay unread server-side and
    // the badge snaps back non-zero after the refetch.
    if (filter === 'all' || filter === 'unread') {
      markAllRead.mutate()
      return
    }
    const ids = visible.filter((n) => !n.is_read).map((n) => n.id)
    if (ids.length > 0) markManyRead.mutate(ids)
  }

  return {
    list,
    unread,
    items,
    unreadCount,
    hasMore,
    fetchOp,
    isFirstLoad,
    isLoadingMore,
    markRead,
    dismiss,
    markAllRead,
    markManyRead,
    markAllReadScoped,
  }
}
