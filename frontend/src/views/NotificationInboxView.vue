<script setup lang="ts">
/**
 * Full-page notification inbox. The bell-popover preview is fine
 * for "what's new at a glance"; the inbox is for triage —
 * scanning a longer feed, batch-clearing, marking stale items
 * read. Lifted to a real route mostly because the popover is
 * cramped on phones, where the inbox is the primary surface.
 *
 * Differences from the bell:
 *   - Bulk selection (checkbox per row + a contextual action bar)
 *   - Finer date grouping (Today / Yesterday / This week / Earlier)
 *   - Infinite scroll via IntersectionObserver — sentinel at the
 *     bottom of the list calls `store.fetchPage(false)` when it
 *     enters the viewport. No "Load more" button, no manual
 *     pagination chrome.
 *
 * Reads from the same `useNotificationsStore` as the bell, so
 * marking-as-read here updates the bell badge live and vice
 * versa.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import type { Notification } from '@nosdesk/core/services/notificationService'
import {
  useArchiveMutation,
  useDeleteManyMutation,
  useMarkAllSeenMutation,
  useMarkUnreadMutation,
  useSnoozeMutation,
  useNotificationsStore,
} from '@/stores/notifications'
import {
  applyNotificationFilter,
  iconForNotificationType,
  useNotificationFeed,
  useNotificationFilterTabs,
  type NotificationFilter,
} from '@/composables/useNotificationFeed'
import { formatInboxTime, parseDate } from '@nosdesk/core/utils/dateUtils'
import NotificationSnoozeMenu from '@/components/NotificationSnoozeMenu.vue'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import Checkbox from '@/components/common/Checkbox.vue'
import PageScroll from '@/components/common/PageScroll.vue'
import AsyncBoundary from '@/components/common/AsyncBoundary.vue'

const router = useRouter()
const store = useNotificationsStore()
const fluent = useFluent()
const tInbox = (key: string, args?: Record<string, string>) => fluent.$t(key, args)
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const filterTabs = useNotificationFilterTabs()

// Shared notification wiring (queries, mutations, derived state,
// presentation helpers). The bell consumes the same composable;
// inbox-specific concerns (bulk select, four-bucket grouping,
// long-form empty copy) stay below.
const feed = useNotificationFeed()
const {
  list,
  items,
  unreadCount,
  hasMore,
  fetchOp,
  isFirstLoad,
  isLoadingMore,
  markRead,
  markManyRead,
} = feed

// Per-item triage: reversible archive (replaces destructive dismiss),
// a read/unread toggle, and snooze.
const archive = useArchiveMutation()
const markUnread = useMarkUnreadMutation()
const snooze = useSnoozeMutation()

// Bulk-delete is inbox-only (no equivalent in the bell), so it
// lives here rather than in the shared feed.
const deleteMany = useDeleteManyMutation()

const filter = ref<NotificationFilter>('all')
const selectedIds = ref<Set<number>>(new Set())
const sentinelRef = ref<HTMLElement | null>(null)
// PageScroll exposes its inner scroll container via defineExpose;
// we pull it through to anchor the IntersectionObserver root
// (the page itself doesn't scroll, the list region does).
const pageScrollRef = ref<InstanceType<typeof PageScroll> | null>(null)
const scrollContainerRef = computed<HTMLElement | null>(
  () => pageScrollRef.value?.scrollContainerRef ?? null,
)
// Anchor for shift-click range selection. Tracks the most
// recent checkbox the user toggled so the next shift-click
// can fill in the range between them, the way every modern
// inbox / file picker handles bulk selection.
const lastClickedId = ref<number | null>(null)

const filteredNotifications = computed(() =>
  applyNotificationFilter(filter.value, items.value),
)

interface NotificationGroup {
  label: string
  items: Notification[]
}

// Inbox uses four buckets vs. the bell's three. The extra "This
// week" tier matters here because the inbox is paginated and a
// user can scroll back further; lumping a week of items into
// "Earlier" makes the chronology feel collapsed.
const groupedNotifications = computed<NotificationGroup[]>(() => {
  const now = new Date()
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  const startOfYesterday = startOfToday - 86_400_000
  const startOfWeek = startOfToday - 6 * 86_400_000
  const today: Notification[] = []
  const yesterday: Notification[] = []
  const week: Notification[] = []
  const earlier: Notification[] = []
  for (const n of filteredNotifications.value) {
    const t = parseDate(n.created_at)?.getTime() ?? 0
    if (t >= startOfToday) today.push(n)
    else if (t >= startOfYesterday) yesterday.push(n)
    else if (t >= startOfWeek) week.push(n)
    else earlier.push(n)
  }
  return [
    { label: t('inbox-group-today'), items: today },
    { label: t('inbox-group-yesterday'), items: yesterday },
    { label: t('inbox-group-this-week'), items: week },
    { label: t('inbox-group-earlier'), items: earlier },
  ].filter((g) => g.items.length > 0)
})

const visibleIds = computed(() => filteredNotifications.value.map((n) => n.id))
const selectedCount = computed(() => selectedIds.value.size)
const hasSelection = computed(() => selectedCount.value > 0)
const isAllSelected = computed(
  () =>
    visibleIds.value.length > 0 &&
    visibleIds.value.every((id) => selectedIds.value.has(id)),
)
const selectedHasUnread = computed(() =>
  filteredNotifications.value.some(
    (n) => selectedIds.value.has(n.id) && !n.is_read,
  ),
)
const visibleHasUnread = computed(() =>
  filteredNotifications.value.some((n) => !n.is_read),
)

const markAllLabel = computed(() => {
  switch (filter.value) {
    case 'mentions':
      return t('inbox-mark-mentions-read')
    case 'unread':
    case 'all':
    default:
      return t('inbox-mark-all-read')
  }
})

// Empty-state copy per filter. A title + supporting sentence
// reads as a refined empty state rather than a stranded label;
// the supporting copy doubles as a hint for what would land in
// each tab so users understand why their feed is quiet.
const emptyContent = computed(() => {
  switch (filter.value) {
    case 'unread':
      return {
        title: t('inbox-empty-caught-up-title'),
        subtitle: t('inbox-empty-caught-up-subtitle'),
      }
    case 'mentions':
      return {
        title: t('inbox-empty-mentions-title'),
        subtitle: t('inbox-empty-mentions-subtitle'),
      }
    case 'all':
    default:
      return {
        title: t('inbox-empty-default-title'),
        subtitle: t('inbox-empty-default-subtitle'),
      }
  }
})

function clearSelection() {
  selectedIds.value = new Set()
  lastClickedId.value = null
}

function toggleSelected(id: number, event?: MouseEvent) {
  event?.stopPropagation()
  // Shift-click range fill: if there's an anchor and the new
  // click landed elsewhere, every item between the two
  // (inclusive) becomes selected. We don't deselect on
  // shift-click — it's an additive gesture, matching Gmail and
  // GitHub behaviour. Plain clicks still toggle.
  if (event?.shiftKey && lastClickedId.value !== null && lastClickedId.value !== id) {
    const ids = visibleIds.value
    const fromIdx = ids.indexOf(lastClickedId.value)
    const toIdx = ids.indexOf(id)
    if (fromIdx >= 0 && toIdx >= 0) {
      const [start, end] = fromIdx < toIdx ? [fromIdx, toIdx] : [toIdx, fromIdx]
      const next = new Set(selectedIds.value)
      for (let i = start; i <= end; i++) next.add(ids[i])
      selectedIds.value = next
      lastClickedId.value = id
      return
    }
  }
  const next = new Set(selectedIds.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  selectedIds.value = next
  lastClickedId.value = id
}

function toggleSelectAll() {
  if (isAllSelected.value) {
    clearSelection()
  } else {
    selectedIds.value = new Set(visibleIds.value)
  }
}

async function navigateToNotification(notification: Notification) {
  if (!notification.is_read) markRead.mutate(notification.id)
  if (notification.entity_type === 'documentation_page') {
    const slug = notification.metadata?.slug as string | undefined
    const pageId = (notification.metadata?.page_id as number | undefined) ?? notification.entity_id
    router.push(`/documentation/${slug || pageId}`)
  } else if (notification.entity_type === 'asset') {
    const assetId = (notification.metadata?.asset_id as number | undefined) ?? notification.entity_id
    router.push(`/assets/${assetId}`)
  } else if (notification.entity_type === 'ticket' || notification.entity_type === 'comment') {
    const ticketId = (notification.metadata?.ticket_id as number | undefined) ?? notification.entity_id
    router.push(`/tickets/${ticketId}`)
  }
}

// Read toggle: one control flips a row between read and unread so a
// user can re-flag an item they want to revisit.
function handleToggleRead(event: Event, notification: Notification) {
  event.stopPropagation()
  if (notification.is_read) markUnread.mutate(notification.id)
  else markRead.mutate(notification.id)
}

function handleSnoozeNotification(until: string, notification: Notification) {
  // Snooze removes the row from the active inbox (like archive) until the
  // chosen time. The snooze menu stops propagation on its own trigger, so
  // no row navigation fires.
  const next = new Set(selectedIds.value)
  next.delete(notification.id)
  selectedIds.value = next
  snooze.mutate({ id: notification.id, until })
}

function handleArchiveNotification(event: Event, notification: Notification) {
  event.stopPropagation()
  // Focus management: keyboard users lose context when a row
  // unmounts under their cursor. Find the next focusable
  // sibling row first; once Vue settles the removal, send
  // focus there. Falls back to the previous sibling when the
  // archived item was the last in its group.
  const trigger = event.currentTarget as HTMLElement | null
  const rowEl = trigger?.closest('[data-notification-row]') as HTMLElement | null
  const target =
    (rowEl?.nextElementSibling as HTMLElement | null) ??
    (rowEl?.previousElementSibling as HTMLElement | null)
  const next = new Set(selectedIds.value)
  next.delete(notification.id)
  selectedIds.value = next
  // Archive (reversible) rather than hard-delete: the row leaves the
  // active inbox but is retained server-side.
  archive.mutate(notification.id)
  nextTick(() => {
    if (!target) return
    const focusable = target.querySelector(
      'button[aria-label^="Dismiss"], button[aria-label^="Mark as read"], button',
    ) as HTMLElement | null
    focusable?.focus()
  })
}

function handleBulkMarkRead() {
  const ids = Array.from(selectedIds.value)
  if (ids.length === 0) return
  markManyRead.mutate(ids)
  clearSelection()
}

function handleBulkDelete() {
  const ids = Array.from(selectedIds.value)
  if (ids.length === 0) return
  deleteMany.mutate(ids)
  clearSelection()
}

function handleMarkAllReadScoped() {
  feed.markAllReadScoped(filter.value, filteredNotifications.value)
}

// IntersectionObserver-driven infinite scroll. Sentinel sits a
// little above the bottom edge so the next page begins loading
// before the user actually hits the floor.
let observer: IntersectionObserver | null = null

function attachObserver() {
  if (!sentinelRef.value || typeof IntersectionObserver === 'undefined') return
  observer?.disconnect()
  observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting && hasMore.value && !isLoadingMore.value) {
          list.loadNextPage()
        }
      }
    },
    {
      // Anchor the observer to our internal scroll container,
      // not the viewport — the page itself doesn't scroll, the
      // list does.
      root: scrollContainerRef.value,
      rootMargin: '400px 0px',
    },
  )
  observer.observe(sentinelRef.value)
}

watch([sentinelRef, scrollContainerRef], attachObserver)

// When selection includes items that get filtered out (e.g.
// switched tab from All to Unread), prune the orphans so the
// bulk bar count reflects what the user can see.
watch(filter, () => {
  if (selectedIds.value.size === 0) return
  const visible = new Set(visibleIds.value)
  const next = new Set<number>()
  for (const id of selectedIds.value) if (visible.has(id)) next.add(id)
  selectedIds.value = next
})

const markAllSeen = useMarkAllSeenMutation()

onMounted(() => {
  // SSE wiring (idempotent). Pinia Colada handles the initial
  // list/unread fetches automatically when the queries mount.
  store.ensureSubscribed()
  // Opening the inbox counts as seeing everything: clear the badge
  // (unseen) without marking items read.
  markAllSeen.mutate()
})

onBeforeUnmount(() => {
  observer?.disconnect()
})
</script>

<template>
  <PageScroll
    ref="pageScrollRef"
    :is-empty="!isFirstLoad && filteredNotifications.length === 0"
  >
    <template #chrome>
    <!-- Page chrome: title + tabs sit above the scroll region
         so they're always visible. Matches the toolbar pattern
         used by TicketsListView / UsersListView / ProjectsView,
         but the inner content is centred and width-constrained
         so list rows stay readable on ultrawide displays. -->
    <header class="flex-shrink-0 border-b border-default bg-surface">
      <div class="mx-auto w-full max-w-5xl px-4 sm:px-6 lg:px-8">
        <div class="flex flex-wrap items-end justify-between gap-3 pb-3 pt-5 sm:pt-6">
          <div class="min-w-0">
            <h1 class="text-xl font-semibold text-primary sm:text-2xl">{{ t('inbox-title') }}</h1>
            <p class="mt-0.5 text-sm text-tertiary">
              {{
                unreadCount > 0
                  ? t('notifications-inbox-unread-count', { count: unreadCount })
                  : t('inbox-no-unread')
              }}
            </p>
          </div>
          <button
            v-if="visibleHasUnread"
            type="button"
            @click="handleMarkAllReadScoped"
            class="rounded-md border border-default bg-surface px-3 py-1.5 text-xs font-medium text-secondary transition-colors hover:bg-surface-hover hover:text-primary"
          >
            {{ markAllLabel }}
          </button>
        </div>
        <div
          role="tablist"
          :aria-label="t('inbox-aria-filter')"
          class="-mx-2 flex items-center gap-1 overflow-x-auto"
        >
          <button
            v-for="tab in filterTabs"
            :key="tab.value"
            type="button"
            role="tab"
            :aria-selected="filter === tab.value"
            @click="filter = tab.value"
            class="relative flex flex-shrink-0 items-center gap-1.5 px-3 py-2.5 text-sm font-medium transition-colors"
            :class="
              filter === tab.value
                ? 'text-primary'
                : 'text-tertiary hover:text-secondary'
            "
          >
            {{ tab.label }}
            <span
              v-if="tab.value === 'unread' && unreadCount > 0"
              class="rounded-full bg-accent/15 px-1.5 py-0.5 text-[11px] font-semibold leading-none text-accent"
            >
              {{ unreadCount > 99 ? '99+' : unreadCount }}
            </span>
            <span
              v-if="filter === tab.value"
              class="absolute inset-x-2 bottom-0 h-0.5 rounded-t bg-accent"
              aria-hidden="true"
            />
          </button>
        </div>
      </div>
    </header>

    <!-- Bulk action bar. Lives in the page chrome (outside the
         scroll region) so it stays anchored as the user scrolls
         through a long selection. -->
    <Transition
      enter-active-class="transition duration-150 ease-out"
      enter-from-class="-translate-y-2 opacity-0"
      enter-to-class="translate-y-0 opacity-100"
      leave-active-class="transition duration-100 ease-in"
      leave-from-class="translate-y-0 opacity-100"
      leave-to-class="-translate-y-2 opacity-0"
    >
      <div
        v-if="hasSelection"
        class="flex-shrink-0 border-b border-default bg-accent/5"
        role="region"
        :aria-label="t('inbox-aria-bulk-actions')"
      >
        <div class="mx-auto flex w-full max-w-5xl items-center justify-between gap-2 px-4 py-2 sm:px-6 lg:px-8">
          <div class="flex items-center gap-3">
            <button
              type="button"
              @click="clearSelection"
              class="rounded p-1 text-tertiary hover:bg-surface-hover hover:text-primary"
              :aria-label="t('inbox-aria-clear-selection')"
            >
              <Icon name="close" size="sm" />
            </button>
            <span class="text-xs font-medium text-primary">
              {{ t('inbox-selected-count', { count: selectedCount }) }}
            </span>
          </div>
          <div class="flex items-center gap-1">
            <button
              type="button"
              :disabled="!selectedHasUnread"
              @click="handleBulkMarkRead"
              class="flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs font-medium text-secondary transition-colors hover:bg-surface-hover hover:text-primary disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Icon name="check" size="xs" />
              {{ t('inbox-action-mark-read') }}
            </button>
            <button
              type="button"
              @click="handleBulkDelete"
              class="flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs font-medium text-status-error transition-colors hover:bg-status-error-muted"
            >
              <Icon name="trash" size="xs" />
              {{ t('inbox-action-delete') }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
    </template>

    <!-- Empty state slot. PageScroll's `#empty` wrapper handles
         full-scroll-width centring; we just supply the inner
         content (icon, title, subtitle). -->
    <template #empty>
      <div class="flex flex-col items-center gap-4 text-center">
        <div
          class="flex h-16 w-16 items-center justify-center rounded-full bg-surface-alt text-tertiary"
          aria-hidden="true"
        >
          <Icon name="bell" size="lg" />
        </div>
        <div class="flex flex-col gap-1 max-w-sm">
          <h3 class="text-base font-semibold text-primary">
            {{ emptyContent.title }}
          </h3>
          <p class="text-sm text-tertiary">{{ emptyContent.subtitle }}</p>
        </div>
      </div>
    </template>

    <!-- Default slot: width-constrained list column. -->
    <div class="overflow-hidden rounded-lg border border-default bg-surface">
          <!-- Select-all + count chrome. Hidden during first
               load (no items yet to select). -->
          <div
            v-if="filteredNotifications.length > 0"
            class="flex items-center gap-3 border-b border-default px-4 py-2"
          >
            <Checkbox
              :model-value="isAllSelected"
              :indeterminate="hasSelection && !isAllSelected"
              size="sm"
              :aria-label="t('inbox-aria-select-all')"
              @change="toggleSelectAll"
            />
            <span class="text-xs text-tertiary">
              {{ filteredNotifications.length }}
              {{ filteredNotifications.length === 1 ? 'notification' : 'notifications' }}
            </span>
          </div>

          <!-- AsyncBoundary owns the first-load state machine.
               The pending slot only renders if the fetch takes
               >300ms AND we have no data — fast loads complete
               invisibly, refetches show through to existing
               data. Global progress bar carries the indicator
               for everything else. -->
          <AsyncBoundary
            :op="fetchOp"
            :has-data="filteredNotifications.length > 0"
            :pending-delay="300"
          >
            <template #pending>
              <!-- Skeleton kept here as the structured placeholder
                   for slow first loads. With the 300ms gate and
                   `prefers-reduced-motion` support on
                   `animate-pulse`, the previous "skeleton flash"
                   pathology is gone. -->
              <div class="flex flex-col gap-px p-2" aria-hidden="true">
                <div
                  v-for="i in 6"
                  :key="i"
                  class="flex animate-pulse items-start gap-3 rounded-md p-3 motion-reduce:animate-none"
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
              <h2
                class="sticky top-0 z-[5] bg-surface/95 px-4 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-tertiary backdrop-blur"
              >
                {{ group.label }}
              </h2>
              <div
                v-for="notification in group.items"
                :key="notification.id"
                data-notification-row
                role="button"
                tabindex="0"
                :aria-label="notification.title"
                @click="navigateToNotification(notification)"
                @keydown.enter="navigateToNotification(notification)"
                @keydown.space.prevent="navigateToNotification(notification)"
                class="group flex cursor-pointer items-start gap-3 border-t border-default px-4 py-3 transition-colors first:border-t-0 hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
                :class="{
                  'bg-accent/5': !notification.is_read,
                  'bg-accent/10 hover:bg-accent/15': selectedIds.has(notification.id),
                }"
              >
                <Checkbox
                  :model-value="selectedIds.has(notification.id)"
                  size="sm"
                  :aria-label="t('inbox-aria-select-item', { title: notification.title })"
                  class="mt-1"
                  @click.stop
                  @change="(e: Event) => toggleSelected(notification.id, e as MouseEvent)"
                />

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
                  <p class="text-sm font-medium text-primary">
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
                      type="button"
                      @click="handleToggleRead($event, notification)"
                      class="rounded p-1 text-tertiary hover:bg-surface-alt hover:text-primary"
                      :aria-label="
                        notification.is_read
                          ? t('inbox-aria-mark-unread', { title: notification.title })
                          : t('inbox-aria-mark-read', { title: notification.title })
                      "
                    >
                      <Icon :name="notification.is_read ? 'eyeOff' : 'check'" size="xs" />
                    </button>
                    <NotificationSnoozeMenu
                      :title="notification.title"
                      @snooze="(until: string) => handleSnoozeNotification(until, notification)"
                    />
                    <button
                      type="button"
                      @click="handleArchiveNotification($event, notification)"
                      class="rounded p-1 text-tertiary hover:bg-surface-alt hover:text-primary"
                      :aria-label="t('inbox-aria-archive', { title: notification.title })"
                    >
                      <Icon name="archive" size="xs" />
                    </button>
                  </div>
                </div>
              </div>
            </section>
            </template>
          </AsyncBoundary>
        </div>

    <!-- Sentinel + status row sit outside the card so the
         "End of feed" / loading text reads as a separator
         rather than a list item. Still inside PageScroll's
         default slot so it sits in the readable column. -->
    <div
      ref="sentinelRef"
      class="flex items-center justify-center py-4 text-xs text-tertiary"
      aria-hidden="true"
    >
      <span v-if="isLoadingMore">{{ t('inbox-footer-loading-more') }}</span>
      <span v-else-if="!hasMore && items.length > 0">{{ t('inbox-footer-end-of-feed') }}</span>
    </div>
  </PageScroll>
</template>
