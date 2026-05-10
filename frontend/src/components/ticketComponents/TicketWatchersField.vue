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
import { computed } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import UserAvatar from '@/components/UserAvatar.vue'
import Icon from '@/components/common/Icon.vue'

const props = defineProps<{
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
</script>

<template>
  <div class="flex flex-col gap-1.5">
    <div class="flex items-center justify-between gap-2">
      <h3 class="text-xs font-medium text-tertiary">
        Watchers
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
        :title="isWatching ? 'Stop watching this ticket' : 'Watch this ticket for updates'"
        :disabled="!currentUserUuid"
        @click="handleToggle"
      >
        <Icon name="bell" class="w-3.5 h-3.5" />
        <span>{{ isWatching ? 'Watching' : 'Watch' }}</span>
      </button>
    </div>

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
        :title="`${overflowCount} more`"
      >+{{ overflowCount }}</span>
    </div>
  </div>
</template>
