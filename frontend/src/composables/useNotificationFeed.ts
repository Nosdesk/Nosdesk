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
import { computed, type ComputedRef, type MaybeRefOrGetter } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  useDismissMutation,
  useMarkAllReadMutation,
  useMarkManyReadMutation,
  useMarkReadMutation,
  useNotificationsList,
  useUnreadCount,
  type NotificationFilter,
} from '@/stores/notifications'
import type { Notification } from '@nosdesk/core/services/notificationService'
import type { IconName } from '@/components/common/icons'
import type { AsyncBoundaryOp } from '@/components/common/AsyncBoundary.vue'

export type { NotificationFilter }

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
  ticket_referenced: 'link',
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
   *  both mean "mark everything read"; "Mentions" clears only
   *  mentions. All three go through the server endpoint so rows
   *  beyond the loaded window are cleared and the badge zeroes. */
  markAllReadScoped: (filter: NotificationFilter) => void
}

/** Feed for one tab. `filter` is a ref or getter so the tab switch
 *  re-keys the list query (each tab is its own server-filtered cache). */
export function useNotificationFeed(filter: MaybeRefOrGetter<NotificationFilter>): NotificationFeed {
  const list = useNotificationsList(filter)
  const unread = useUnreadCount()

  const markRead = useMarkReadMutation()
  const dismiss = useDismissMutation()
  const markAllRead = useMarkAllReadMutation()
  const markManyRead = useMarkManyReadMutation()

  const items = computed<Notification[]>(
    () => list.data.value?.pages.flatMap((p) => p.items) ?? [],
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

  function markAllReadScoped(filter: NotificationFilter) {
    markAllRead.mutate(filter === 'mentions' ? 'mentioned' : undefined)
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
