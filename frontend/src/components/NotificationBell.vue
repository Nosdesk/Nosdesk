<script setup lang="ts">
/**
 * Header notification preview. Renders the bell trigger + badge
 * and a `<ResponsiveMenu>` panel underneath (popover at md+,
 * bottom sheet on touch). The full feed lives at `/inbox`; this
 * surface is the at-a-glance preview.
 *
 *   1. Filter tabs — All / Unread / Mentions. Pure derived state
 *      over the loaded page; switching tabs is instant. Matches
 *      Linear / Notion / GitHub conventions.
 *   2. Date grouping — Today / Yesterday / Earlier. Lossless
 *      time orientation without timestamping every row.
 *   3. Per-item actions — mark-as-read (unread only) + dismiss.
 *      Visible at low opacity on touch (no hover state) and
 *      brighten on hover at desktop sizes.
 *
 * Data comes from `useNotificationsStore` so the inbox view and
 * this preview always agree on what's read, what's unread, and
 * the badge count. SSE is wired up by the store via
 * `ensureSubscribed`; multiple consumers can call it safely.
 */
import { computed, onMounted, ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useRouter } from 'vue-router'
import type { Notification } from '@/services/notificationService'
import { useNotificationsStore } from '@/stores/notifications'
import {
  applyNotificationFilter,
  iconForNotificationType,
  NOTIFICATION_FILTER_TABS,
  useNotificationFeed,
  type NotificationFilter,
} from '@/composables/useNotificationFeed'
import { formatInboxTime, parseDate } from '@/utils/dateUtils'
import { useFluent } from 'fluent-vue'
import ResponsiveMenu from './common/ResponsiveMenu.vue'
import Icon from './common/Icon.vue'
import AsyncBoundary from './common/AsyncBoundary.vue'
import UnreadBadge from './common/UnreadBadge.vue'
import type { PopoverAnchor } from '@/composables/usePopover'

const router = useRouter()
const store = useNotificationsStore()
const { lastAnnouncement } = storeToRefs(store)

// Passed into formatInboxTime so "Yesterday at" / "Mon at"
// connectors come from the active locale's FTL catalogue.
const fluent = useFluent()
const tInbox = (key: string, args?: Record<string, string>) => fluent.$t(key, args)

// Shared notification wiring (queries, mutations, derived state,
// presentation helpers). The bell and the inbox both consume
// this composable; surface-specific concerns (layout, empty-state
// copy, date-grouping granularity) stay in each view.
const feed = useNotificationFeed()
const {
  list,
  unread,
  items,
  unreadCount,
  hasMore,
  fetchOp,
  isLoadingMore,
  markRead,
  dismiss,
} = feed

const buttonRef = ref<HTMLButtonElement | null>(null)
const isOpen = ref(false)
const filter = ref<NotificationFilter>('all')

const anchor = computed<PopoverAnchor>(() => ({
  type: 'element',
  element: () => buttonRef.value,
}))

const hasUnread = computed(() => unreadCount.value > 0)
const displayCount = computed(() => (unreadCount.value > 99 ? '99+' : String(unreadCount.value)))

const filteredNotifications = computed(() =>
  applyNotificationFilter(filter.value, items.value),
)

interface NotificationGroup {
  label: string
  items: Notification[]
}

const groupedNotifications = computed<NotificationGroup[]>(() => {
  const now = new Date()
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  const startOfYesterday = startOfToday - 86_400_000
  const today: Notification[] = []
  const yesterday: Notification[] = []
  const earlier: Notification[] = []
  for (const n of filteredNotifications.value) {
    // parseDate handles backend NaiveDateTime correctly (UTC).
    const t = parseDate(n.created_at)?.getTime() ?? 0
    if (t >= startOfToday) today.push(n)
    else if (t >= startOfYesterday) yesterday.push(n)
    else earlier.push(n)
  }
  return [
    { label: 'Today', items: today },
    { label: 'Yesterday', items: yesterday },
    { label: 'Earlier', items: earlier },
  ].filter((g) => g.items.length > 0)
})

// Empty-state copy mirrors the inbox view so the surfaces feel
// like one product. Compact wording — the popover is narrow and
// readers shouldn't have to scan past two lines.
const emptyContent = computed(() => {
  switch (filter.value) {
    case 'unread':
      return {
        title: "You're all caught up",
        subtitle: 'New notifications will appear here.',
      }
    case 'mentions':
      return {
        title: 'No mentions yet',
        subtitle: "@mentions in comments will show up here.",
      }
    case 'all':
    default:
      return {
        title: 'No notifications yet',
        subtitle: 'Updates from your tickets, mentions, and docs will land here.',
      }
  }
})

function toggleOpen() {
  if (isOpen.value) {
    isOpen.value = false
    return
  }
  isOpen.value = true
  // Refresh on open to surface any quietly-arrived items the
  // SSE handler may have invalidated. Colada serves cached data
  // immediately and refetches in the background.
  list.refresh()
  unread.refresh()
}

function close() {
  isOpen.value = false
}

async function navigateToNotification(notification: Notification) {
  if (!notification.is_read) markRead.mutate(notification.id)
  close()
  if (notification.entity_type === 'documentation_page') {
    const slug = notification.metadata?.slug as string | undefined
    const pageId = (notification.metadata?.page_id as number | undefined) ?? notification.entity_id
    router.push(`/documentation/${slug || pageId}`)
  } else if (notification.entity_type === 'ticket' || notification.entity_type === 'comment') {
    const ticketId = (notification.metadata?.ticket_id as number | undefined) ?? notification.entity_id
    router.push(`/tickets/${ticketId}`)
  }
}

function handleMarkRead(event: Event, notification: Notification) {
  event.stopPropagation()
  if (!notification.is_read) markRead.mutate(notification.id)
}

function handleClearNotification(event: Event, notification: Notification) {
  event.stopPropagation()
  dismiss.mutate(notification.id)
}

function handleViewInbox() {
  close()
  router.push('/inbox')
}

function handleMarkAllReadScoped() {
  feed.markAllReadScoped(filter.value, filteredNotifications.value)
}

const visibleHasUnread = computed(() =>
  filteredNotifications.value.some((n) => !n.is_read),
)

onMounted(() => {
  store.ensureSubscribed()
})
</script>

<template>
  <div>
    <!-- Polite live region for screen readers. Bell is always
         mounted on every authed page so this is the right host
         for the announcement. Visually hidden, never focused. -->
    <div
      role="status"
      aria-live="polite"
      aria-atomic="true"
      class="sr-only"
    >
      {{ lastAnnouncement }}
    </div>

    <button
      ref="buttonRef"
      type="button"
      @click="toggleOpen"
      class="relative rounded-lg p-2 text-secondary transition-colors hover:bg-surface-hover hover:text-primary focus:outline-none focus:ring-2 focus:ring-accent"
      aria-label="Notifications"
      :aria-expanded="isOpen"
    >
      <Icon name="bell" size="md" />
      <UnreadBadge :count="unreadCount" class="absolute -right-0.5 -top-0.5" />
    </button>

    <ResponsiveMenu
      :open="isOpen"
      :anchor="anchor"
      placement="bottom-end"
      :offset="8"
      react-to-scroll="reposition"
      :auto-focus="false"
      role="dialog"
      aria-label="Notifications"
      popover-class="flex w-[380px] max-h-[520px] flex-col overflow-hidden rounded-xl border border-default bg-surface shadow-xl"
      @close="close"
    >
      <!-- Header. "Open inbox" is given primary placement
           (right-aligned button next to the title) so users
           don't have to scan to a footer link to reach the
           full-page surface. -->
      <header class="flex flex-shrink-0 items-center justify-between gap-2 border-b border-default px-4 py-3">
        <h3 class="text-sm font-semibold text-primary">Notifications</h3>
        <button
          type="button"
          v-prefetch="'/inbox'"
          @click="handleViewInbox"
          class="flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium text-secondary transition-colors hover:bg-surface-hover hover:text-primary"
        >
          Open inbox
          <Icon name="openExternal" size="xs" />
        </button>
      </header>

      <div
        role="tablist"
        aria-label="Filter notifications"
        class="flex flex-shrink-0 items-center gap-1 border-b border-default px-2"
      >
        <button
          v-for="tab in NOTIFICATION_FILTER_TABS"
          :key="tab.value"
          type="button"
          role="tab"
          :aria-selected="filter === tab.value"
          @click="filter = tab.value"
          class="relative flex items-center justify-center gap-1.5 px-4 sm:px-3 min-h-[44px] sm:min-h-0 sm:py-2 text-xs font-medium transition-colors"
          :class="
            filter === tab.value
              ? 'text-primary'
              : 'text-tertiary hover:text-secondary'
          "
        >
          {{ tab.label }}
          <span
            v-if="tab.value === 'unread' && hasUnread"
            class="rounded-full bg-accent/15 px-1.5 py-0.5 text-[10px] font-semibold leading-none text-accent"
          >
            {{ displayCount }}
          </span>
          <span
            v-if="filter === tab.value"
            class="absolute inset-x-2 bottom-0 h-0.5 rounded-t bg-accent"
            aria-hidden="true"
          />
        </button>
      </div>

      <div class="flex-1 overflow-y-auto">
        <!-- Empty state when there's truly nothing AND we're
             not actively loading. Lifted out of AsyncBoundary
             since "no items after a successful fetch" is a
             data-shape concern, not a lifecycle one. -->
        <div
          v-if="!fetchOp.isPending && filteredNotifications.length === 0"
          class="flex flex-col items-center justify-center gap-3 px-6 py-12 text-center"
        >
          <div
            class="flex h-12 w-12 items-center justify-center rounded-full bg-surface-alt text-tertiary"
            aria-hidden="true"
          >
            <Icon name="bell" size="md" />
          </div>
          <div class="flex flex-col gap-0.5 max-w-[14rem]">
            <p class="text-sm font-medium text-primary">{{ emptyContent.title }}</p>
            <p class="text-xs text-tertiary">{{ emptyContent.subtitle }}</p>
          </div>
        </div>

        <AsyncBoundary
          v-else
          :op="fetchOp"
          :has-data="filteredNotifications.length > 0"
          :pending-delay="300"
        >
          <template #pending>
            <div class="flex flex-col gap-px p-2" aria-hidden="true">
              <div
                v-for="i in 4"
                :key="i"
                class="flex animate-pulse items-start gap-3 rounded-md p-2 motion-reduce:animate-none"
              >
                <div class="h-8 w-8 flex-shrink-0 rounded-full bg-surface-alt"></div>
                <div class="flex-1 flex flex-col gap-2 py-1">
                  <div class="h-3 w-3/4 rounded bg-surface-alt"></div>
                  <div class="h-3 w-1/2 rounded bg-surface-alt"></div>
                </div>
              </div>
            </div>
          </template>

          <template #default>
          <section
            v-for="group in groupedNotifications"
            :key="group.label"
            class="border-b border-default last:border-b-0"
          >
            <h4
              class="sticky top-0 z-10 bg-surface/95 px-4 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-tertiary backdrop-blur"
            >
              {{ group.label }}
            </h4>
            <button
              v-for="notification in group.items"
              :key="notification.id"
              type="button"
              @click="navigateToNotification(notification)"
              class="group flex w-full items-start gap-3 px-4 py-3 text-left transition-colors hover:bg-surface-hover"
              :class="{ 'bg-accent/5': !notification.is_read }"
            >
              <div
                class="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full"
                :class="
                  notification.is_read
                    ? 'bg-surface-alt text-tertiary'
                    : 'bg-accent/10 text-accent'
                "
              >
                <Icon :name="iconForNotificationType(notification.notification_type)" size="sm" />
              </div>

              <div class="min-w-0 flex-1">
                <p class="line-clamp-1 text-sm font-medium text-primary">
                  {{ notification.title }}
                </p>
                <p
                  v-if="notification.body"
                  class="mt-0.5 line-clamp-2 text-xs text-secondary"
                >
                  {{ notification.body }}
                </p>
                <p class="mt-1 text-xs text-tertiary">
                  {{ formatInboxTime(notification.created_at, tInbox) }}
                </p>
              </div>

              <div class="flex flex-shrink-0 flex-col items-end gap-1.5">
                <span
                  v-if="!notification.is_read"
                  class="h-2 w-2 rounded-full bg-accent"
                  aria-hidden="true"
                />
                <div
                  class="flex items-center gap-0.5 opacity-60 transition-opacity group-hover:opacity-100"
                >
                  <button
                    v-if="!notification.is_read"
                    type="button"
                    @click="handleMarkRead($event, notification)"
                    class="inline-flex items-center justify-center rounded p-1 text-tertiary hover:bg-surface-alt hover:text-primary touch-target"
                    :aria-label="`Mark as read: ${notification.title}`"
                  >
                    <Icon name="check" size="xs" />
                  </button>
                  <button
                    type="button"
                    @click="handleClearNotification($event, notification)"
                    class="inline-flex items-center justify-center rounded p-1 text-tertiary hover:bg-surface-alt hover:text-primary touch-target"
                    :aria-label="`Dismiss: ${notification.title}`"
                  >
                    <Icon name="close" size="xs" />
                  </button>
                </div>
              </div>
            </button>
          </section>

          <div
            v-if="hasMore"
            class="flex items-center justify-center border-t border-default p-3"
          >
            <button
              type="button"
              @click="list.loadNextPage()"
              :disabled="isLoadingMore"
              class="text-xs font-medium text-accent hover:text-accent-hover disabled:cursor-not-allowed disabled:opacity-50"
            >
              {{ isLoadingMore ? 'Loading...' : 'Load more' }}
            </button>
          </div>
          </template>
        </AsyncBoundary>
      </div>

      <!-- Footer. Header now carries the primary "Open inbox"
           affordance, so the footer is left for "Mark all read"
           (filter-scoped) and the settings escape hatch. -->
      <footer class="flex flex-shrink-0 items-center justify-between gap-2 border-t border-default px-4 py-2">
        <button
          v-if="visibleHasUnread"
          type="button"
          @click="handleMarkAllReadScoped"
          class="text-xs font-medium text-accent hover:text-accent-hover"
        >
          {{ filter === 'mentions' ? 'Mark mentions as read' : 'Mark all as read' }}
        </button>
        <span v-else />
        <router-link
          to="/profile/settings/notifications"
          @click="close"
          class="text-xs font-medium text-tertiary hover:text-primary"
        >
          Settings
        </router-link>
      </footer>
    </ResponsiveMenu>
  </div>
</template>
