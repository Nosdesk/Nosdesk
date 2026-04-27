/**
 * Shared notification feed.
 *
 * Two surfaces consume this store: the header `<NotificationBell>`
 * (preview popover / bottom sheet) and `<NotificationInboxView>`
 * (full-page inbox). Both render slices of the same paginated
 * feed and call the same mutations, so reading an item in one
 * surface immediately updates the other (and the bell badge)
 * without an extra refetch.
 *
 * The feed is the most-recent page only — the store does not
 * try to be a long-lived offline cache. New items arriving over
 * SSE trigger a first-page refetch instead of a local prepend
 * because the server does pagination, ordering, and filtering
 * authoritatively.
 *
 * SSE wiring is owned here (idempotent `ensureSubscribed`) so
 * consumers don't each redundantly attach listeners; the bell
 * is always mounted, but the inbox can come and go.
 */
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  deleteNotifications,
  getNotifications,
  getUnreadCount,
  markAllNotificationsRead,
  markNotificationsRead,
  type Notification,
} from '@/services/notificationService'
import { useSSE } from '@/services/sseService'
import type { NotificationReceivedEventData } from '@/types/sse'
import { unwrapEventData } from '@/types/sse'
import { useAuthStore } from './auth'

const PAGE_SIZE = 20

export const useNotificationsStore = defineStore('notifications', () => {
  const items = ref<Notification[]>([])
  const unreadCount = ref(0)
  const isInitialLoading = ref(false)
  const isLoadingMore = ref(false)
  const hasMore = ref(false)
  // Aria-live announcement target. Bumped when a new SSE
  // notification arrives so screen-reader users hear it. The
  // counter suffix forces Vue to treat each set as a new
  // mutation even when two notifications share a title.
  const lastAnnouncement = ref('')
  let announcementSeq = 0
  let subscribed = false

  // True only when there's truly nothing to show. Once the first
  // page has loaded, refetches happen against existing data so
  // we never flash an empty list back to the user.
  const isFirstLoad = computed(
    () => isInitialLoading.value && items.value.length === 0,
  )

  async function fetchPage(reset = false) {
    try {
      if (reset) {
        isInitialLoading.value = true
      } else {
        if (isLoadingMore.value || !hasMore.value) return
        isLoadingMore.value = true
      }
      const offset = reset ? 0 : items.value.length
      const [batch, count] = await Promise.all([
        getNotifications({ limit: PAGE_SIZE, offset }),
        reset ? getUnreadCount() : Promise.resolve(unreadCount.value),
      ])
      items.value = reset ? batch : [...items.value, ...batch]
      hasMore.value = batch.length === PAGE_SIZE
      if (reset) unreadCount.value = count
    } catch (error) {
      console.error('Failed to fetch notifications:', error)
    } finally {
      isInitialLoading.value = false
      isLoadingMore.value = false
    }
  }

  async function markRead(ids: number[]) {
    if (ids.length === 0) return
    try {
      await markNotificationsRead(ids)
      const idSet = new Set(ids)
      let unreadFlipped = 0
      items.value.forEach((n) => {
        if (idSet.has(n.id) && !n.is_read) {
          n.is_read = true
          unreadFlipped++
        }
      })
      unreadCount.value = Math.max(0, unreadCount.value - unreadFlipped)
    } catch (error) {
      console.error('Failed to mark notifications as read:', error)
    }
  }

  async function markAllRead() {
    try {
      await markAllNotificationsRead()
      items.value.forEach((n) => (n.is_read = true))
      unreadCount.value = 0
    } catch (error) {
      console.error('Failed to mark all as read:', error)
    }
  }

  async function deleteItems(ids: number[]) {
    if (ids.length === 0) return
    try {
      await deleteNotifications(ids)
      const idSet = new Set(ids)
      let unreadRemoved = 0
      items.value = items.value.filter((n) => {
        if (!idSet.has(n.id)) return true
        if (!n.is_read) unreadRemoved++
        return false
      })
      unreadCount.value = Math.max(0, unreadCount.value - unreadRemoved)
    } catch (error) {
      console.error('Failed to delete notifications:', error)
    }
  }

  function ensureSubscribed() {
    if (subscribed) return
    subscribed = true
    const { addEventListener, connect, isConnected } = useSSE()
    if (!isConnected.value) connect()
    addEventListener('notification-received', handleSseEvent)
  }

  function handleSseEvent(rawData: unknown) {
    try {
      const data = unwrapEventData(rawData as NotificationReceivedEventData)
      const auth = useAuthStore()
      if (!auth.user?.uuid || auth.user.uuid !== data.recipient_uuid) return
      unreadCount.value++
      // Announce the arrival to screen-reader users. Polite live
      // region elsewhere in the tree picks this up.
      announcementSeq++
      const title = data.notification?.title?.trim()
      lastAnnouncement.value = title
        ? `New notification: ${title} (${announcementSeq})`
        : `New notification (${announcementSeq})`
      // Refresh the first page so the new item slots in at the
      // top with correct ordering and metadata. Cheaper than
      // attempting to prepend client-side and racing against
      // the next pagination call.
      fetchPage(true)
    } catch (error) {
      console.error('Error handling notification SSE event:', error)
    }
  }

  return {
    items,
    unreadCount,
    isInitialLoading,
    isLoadingMore,
    isFirstLoad,
    hasMore,
    lastAnnouncement,
    fetchPage,
    markRead,
    markAllRead,
    deleteItems,
    ensureSubscribed,
  }
})
