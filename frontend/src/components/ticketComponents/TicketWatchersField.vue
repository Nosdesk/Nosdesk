<script setup lang="ts">
/**
 * Watchers sidebar surface — the "I want to be told about this
 * ticket without owning it" affordance.
 *
 * Layout:
 *   Watchers (3)                     [🔔 Watching] [⚙]
 *   [avatar] [avatar] [avatar] +5
 *
 * Three controls, each with one job:
 *   - Bell toggle (label + count + bell glyph) — toggles the
 *     current user's subscription. Single click.
 *   - Settings icon (only when watching) — opens a popover with
 *     per-watch preferences (currently: notify-on-internal-notes).
 *     Convergent with GitHub's split bell+chevron pattern.
 *   - Avatar pile below — visual roster, with +N overflow when
 *     the set grows past 5.
 *
 * Earlier revisions of this component carried the preference as
 * a full-width chip-with-ON/OFF-pill below the bell, which read
 * as a second competing primary control in a tiny region. Moving
 * it into a popover restores the "one control per row type" rule
 * (Linear / Plain / Front pattern) and recovers ~30px of sidebar
 * height for the average ticket that doesn't need to inspect the
 * preference.
 *
 * Comment-notification fan-out happens server-side: backend reads
 * the watcher set when a comment lands and notifies every uuid
 * (deduped against the requester / assignee / @mentions). This
 * component just exposes the toggle and the preference.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useAuthStore } from '@/stores/auth'
import { watcherService } from '@/services/watcherService'
import UserAvatar from '@/components/UserAvatar.vue'
import Icon from '@/components/common/Icon.vue'
import Popover from '@/components/common/Popover.vue'

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

// Preference popover. Trigger renders only when the current user
// is watching; closes the popover automatically when the user
// stops watching (the trigger disappears mid-flow).
const prefsButtonRef = ref<HTMLElement | null>(null)
const prefsOpen = ref(false)

const prefsAnchor = computed(() => ({
  type: 'element' as const,
  element: () => prefsButtonRef.value,
}))

watch(isWatching, (watching) => {
  if (!watching) prefsOpen.value = false
})
</script>

<template>
  <!-- Single-row component now (avatars inline with heading);
       wrapper exists for the Popover slot. No internal gap to
       apply, but the flex-col scaffolding stays so future state
       (per-watch preference summary, etc.) can be added inline
       without restructuring. -->
  <div class="flex flex-col">
    <!-- Single-row layout: label + inline avatar pile on the left,
         bell toggle + popover trigger on the right. Avatars sit
         inline rather than wrapping to their own row so Watchers
         shares vertical footprint with the other Relations rows
         (no orphan-avatar block). `flex-wrap` on the left side
         lets the pile drop below the label only when the sidebar
         is narrow enough to force it. `-mx-2 px-2` matches the
         outer extent of PropertyChipRow / TicketTagsField so every
         property heading shares one box geometry.

         Vertical alignment: outer `items-center` keeps the heading
         text aligned with the bell-toggle baseline, and the
         `min-h-6` on the left container locks its height to 24px
         regardless of whether avatars are rendered (without this,
         the inner container grew by 4px when avatars rendered and
         the centered h3 visibly shifted down by 2px). -->
    <div class="flex items-center justify-between gap-2 -mx-2 px-2">
      <div class="flex items-center gap-2 flex-wrap min-w-0 min-h-6">
        <h3 class="text-xs font-medium text-tertiary shrink-0">
          {{ t('ticket-field-watchers-label') }}
          <span v-if="watcherCount > 0" class="text-tertiary tabular-nums">({{ watcherCount }})</span>
        </h3>
        <!-- Inline avatar pile. Skip entirely on empty so the label
             stands alone in the dominant "no watchers" case. -->
        <div
          v-if="watcherCount > 0"
          class="flex items-center gap-1"
        >
          <UserAvatar
            v-for="uuid in visibleAvatars"
            :key="uuid"
            :uuid="uuid"
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
      <div class="flex items-center gap-0.5 shrink-0">
        <!-- Bell toggle. Same bell glyph for both states; colour +
             weight distinguishes them (no `bellOff` in the registry).
             Subscribed reads as a quiet accent text colour with a
             hover tint, not a permanent chip background, so the
             row sits at the same visual weight as sibling property
             rows (the previous amber pill stood out against the
             flat panel). aria-pressed is the canonical toggle
             accessibility primitive. -->
        <button
          type="button"
          class="inline-flex items-center gap-1 px-2 h-6 rounded text-[11px] font-medium transition-colors"
          :class="isWatching
            ? 'text-accent hover:bg-accent-muted'
            : 'text-tertiary hover:text-primary hover:bg-surface-hover'"
          :aria-pressed="isWatching"
          :title="isWatching ? t('ticket-field-watchers-unwatch-title') : t('ticket-field-watchers-watch-title')"
          :disabled="!currentUserUuid"
          @click="handleToggle"
        >
          <Icon name="bell" class="w-3.5 h-3.5" />
          <span>{{ isWatching ? t('ticket-field-watchers-watching') : t('ticket-field-watchers-watch') }}</span>
        </button>
        <!-- Per-watch preferences. Only meaningful when watching;
             hidden otherwise so the bell row stays minimal for the
             dominant "not subscribed" case. GitHub's split bell
             pattern: primary toggle on the left, preferences
             chevron on the right. -->
        <button
          v-if="isWatching"
          ref="prefsButtonRef"
          type="button"
          class="inline-flex items-center justify-center w-6 h-6 rounded text-tertiary hover:text-primary hover:bg-surface-hover transition-colors"
          :aria-haspopup="true"
          :aria-expanded="prefsOpen"
          :title="t('ticket-field-watchers-prefs-title')"
          @click="prefsOpen = !prefsOpen"
        >
          <Icon name="chevronDown" class="w-3 h-3" />
        </button>
      </div>
    </div>

    <!-- Preferences popover. Anchored to the chevron button next
         to the bell. Per-watch only; the user's global notification
         settings still own digest mode, mute-everything, etc. -->
    <Popover
      :open="prefsOpen"
      :anchor="prefsAnchor"
      placement="bottom-end"
      role="dialog"
      :aria-label="t('ticket-field-watchers-prefs-title')"
      popover-class="bg-surface border border-default rounded-lg overflow-hidden min-w-[220px]"
      @close="prefsOpen = false"
    >
      <div class="p-1 flex flex-col gap-0.5">
        <button
          type="button"
          class="flex items-center justify-between gap-2 px-2 py-1.5 rounded text-left hover:bg-surface-hover transition-colors"
          :aria-pressed="notifyOnInternalNotes"
          @click="toggleInternalNotify"
        >
          <span class="flex flex-col">
            <span class="text-xs text-primary">{{ t('ticket-field-watchers-notify-internal') }}</span>
            <span class="text-[10px] text-tertiary">{{ t('ticket-field-watchers-notify-internal-hint') }}</span>
          </span>
          <Icon
            v-if="notifyOnInternalNotes"
            name="check"
            class="w-3.5 h-3.5 text-accent shrink-0"
            aria-hidden="true"
          />
          <span
            v-else
            class="w-3.5 h-3.5 shrink-0"
            aria-hidden="true"
          />
        </button>
        <p
          v-if="prefError"
          class="px-2 py-1 text-[10px] text-status-error"
          role="alert"
        >
          {{ prefError }}
        </p>
      </div>
    </Popover>
  </div>
</template>
