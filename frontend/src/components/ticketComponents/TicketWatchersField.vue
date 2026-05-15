<script setup lang="ts">
/**
 * Watchers sidebar surface — the "I want to be told about this
 * ticket without owning it" affordance.
 *
 * Two pieces:
 *  - A bell toggle for the current user (filled when watching).
 *    Mirrors the pattern Linear / GitHub / Jira use; one click
 *    starts or stops the subscription.
 *  - A list of avatars for everyone watching, with overflow
 *    counter when the set grows past 5. The avatars are linked
 *    to user profiles via the standard UserAvatar component.
 *
 * Comment-notification fan-out happens server-side: backend reads
 * the watcher set when a comment lands and notifies every uuid
 * (deduped against the requester / assignee / @mentions). This
 * component just exposes the toggle.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useAuthStore } from '@/stores/auth'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import { watcherService } from '@/services/watcherService'
import UserAvatar from '@/components/UserAvatar.vue'
import Icon from '@/components/common/Icon.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{
  ticketId: number
  watcherUuids: string[]
}>()

const emit = defineEmits<{
  (e: 'toggle'): void
}>()

const authStore = useAuthStore()
const { getUserHandle } = useUsersDirectory()

const currentUserUuid = computed<string | null>(() => authStore.user?.uuid ?? null)

const isWatching = computed<boolean>(() => {
  const uuid = currentUserUuid.value
  if (!uuid) return false
  return props.watcherUuids.includes(uuid)
})

const watcherCount = computed<number>(() => props.watcherUuids.length)

// Show up to 5 avatars; render +N overflow indicator after that.
// 5 fits the sidebar comfortably without wrapping; bigger sets
// rely on the count + the (future) "all watchers" modal.
const visibleAvatars = computed<string[]>(() => props.watcherUuids.slice(0, 5))
const overflowCount = computed<number>(() => Math.max(0, props.watcherUuids.length - 5))

function handleToggle() {
  if (!currentUserUuid.value) return
  emit('toggle')
}

// Per-watch preference: mute internal-note notifications. Loaded
// on demand whenever the current user is watching; cleared back to
// true (the server default) when they unwatch so the next watch
// starts fresh. Optimistic write so the toggle reads correctly
// even before the PATCH round-trip resolves; reverted on error.
const notifyOnInternalNotes = ref<boolean>(true)
const prefError = ref<string | null>(null)

async function refreshMyWatchPref() {
  if (!isWatching.value) {
    notifyOnInternalNotes.value = true
    prefError.value = null
    return
  }
  try {
    const state = await watcherService.myState(props.ticketId)
    notifyOnInternalNotes.value = state.notify_on_internal_notes
    prefError.value = null
  } catch (e) {
    prefError.value = e instanceof Error ? e.message : t('ticket-field-watchers-pref-load-error')
  }
}

watch(
  () => [props.ticketId, isWatching.value] as const,
  () => {
    void refreshMyWatchPref()
  },
  { immediate: true },
)

async function toggleInternalNotify() {
  const previous = notifyOnInternalNotes.value
  const next = !previous
  notifyOnInternalNotes.value = next
  prefError.value = null
  try {
    await watcherService.updatePreferences(props.ticketId, {
      notify_on_internal_notes: next,
    })
  } catch (e) {
    notifyOnInternalNotes.value = previous
    prefError.value = e instanceof Error ? e.message : t('ticket-field-watchers-pref-save-error')
  }
}
</script>

<template>
  <div class="flex flex-col gap-1.5">
    <!-- Heading row uses the same `-mx-2 px-2` outer extent as
         the interactive button rows in PropertyChipRow /
         TicketTagsField, so every property heading shares one
         box geometry — guaranteed pixel-perfect alignment of
         the label text with button-style siblings. -->
    <div class="flex items-center justify-between gap-2 -mx-2 px-2">
      <h3 class="text-xs font-medium text-tertiary">
        {{ t('ticket-field-watchers-label') }}
        <span v-if="watcherCount > 0" class="text-tertiary tabular-nums">({{ watcherCount }})</span>
      </h3>
      <!-- Bell toggle. Filled bell = watching; outlined = not.
           No `bellOff` icon in the registry today, so we lean on
           colour + semibold weight + the same bell glyph for
           both states. The aria-pressed flip is the canonical
           accessibility primitive for a toggle button. -->
      <button
        type="button"
        class="inline-flex items-center gap-1 px-2 h-6 rounded text-[11px] font-medium transition-colors"
        :class="isWatching
          ? 'bg-accent-muted text-accent'
          : 'text-tertiary hover:text-primary hover:bg-surface-hover'"
        :aria-pressed="isWatching"
        :title="isWatching ? t('ticket-field-watchers-unwatch-title') : t('ticket-field-watchers-watch-title')"
        :disabled="!currentUserUuid"
        @click="handleToggle"
      >
        <Icon name="bell" class="w-3.5 h-3.5" />
        <span>{{ isWatching ? t('ticket-field-watchers-watching') : t('ticket-field-watchers-watch') }}</span>
      </button>
    </div>

    <!-- Per-watch visibility preference. Rendered only when the
         current user is watching; lets a staff watcher follow the
         public conversation without being pinged for every internal
         note. Mentions still notify (those are explicit pings,
         not implicit fan-out), so the copy specifically calls out
         "internal notes" rather than a broader "mute" framing. -->
    <button
      v-if="isWatching"
      type="button"
      class="-mx-2 px-2 py-1 inline-flex items-center justify-between gap-2 text-[11px] text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
      :aria-pressed="!notifyOnInternalNotes"
      @click="toggleInternalNotify"
    >
      <span class="flex items-center gap-1.5">
        <Icon
          :name="notifyOnInternalNotes ? 'bell' : 'bell'"
          class="w-3 h-3 flex-shrink-0"
          :class="notifyOnInternalNotes ? '' : 'opacity-40'"
        />
        <span class="truncate">
          {{ notifyOnInternalNotes ? t('ticket-field-watchers-notify-internal') : t('ticket-field-watchers-public-only') }}
        </span>
      </span>
      <span
        class="inline-flex items-center justify-center h-4 px-1.5 rounded-full text-[9px] font-medium"
        :class="notifyOnInternalNotes
          ? 'bg-accent-muted text-accent'
          : 'bg-surface-alt text-tertiary'"
      >
        {{ notifyOnInternalNotes ? t('ticket-field-watchers-toggle-on') : t('ticket-field-watchers-toggle-off') }}
      </span>
    </button>
    <p
      v-if="prefError"
      class="text-[10px] text-status-error -mt-1"
      role="alert"
    >
      {{ prefError }}
    </p>

    <!-- Avatar row. Empty state: skip the row entirely so the
         "Watchers (0)" + bell button stand alone. Avoids a
         visually empty container that reads as broken. -->
    <div
      v-if="watcherCount > 0"
      class="flex items-center gap-1 flex-wrap"
    >
      <UserAvatar
        v-for="uuid in visibleAvatars"
        :key="uuid"
        :name="uuid"
        :user-name="getUserHandle(uuid).user.value?.name ?? undefined"
        :avatar="getUserHandle(uuid).user.value?.avatar_thumb || getUserHandle(uuid).user.value?.avatar_url || null"
        size="xs"
        :show-name="false"
        :clickable="true"
      />
      <span
        v-if="overflowCount > 0"
        class="inline-flex items-center justify-center h-5 min-w-[1.25rem] px-1 rounded-full bg-surface-alt text-tertiary text-[10px] font-medium"
        :title="t('ticket-field-watchers-overflow-title', { count: overflowCount })"
      >+{{ overflowCount }}</span>
    </div>
  </div>
</template>
