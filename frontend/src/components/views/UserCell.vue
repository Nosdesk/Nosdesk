<script setup lang="ts">
/**
 * Compact user cell for data tables. Resolves a user UUID
 * through the shared directory composable and renders an
 * avatar + truncated name. Degrades gracefully through three
 * states:
 *
 *   1. uuid is null               -> em-dash
 *   2. uuid is known, user not yet in cache -> avatar with
 *      '?' initials, name shows '...' so the row stays the
 *      same height during fetch
 *   3. user resolved              -> avatar (image or coloured
 *      initials) + display name
 *
 * Each instance is its own reactive scope so the cell updates
 * in place when the dataStore cache populates — even when the
 * parent table row is wrapped in v-memo. v-memo skips render
 * of the parent subtree, but the inner component's own
 * reactivity continues to track its computeds.
 */
import { computed } from 'vue'
import UserAvatar from '@/components/UserAvatar.vue'
import { useUsersDirectory } from '@/composables/useUsersDirectory'

const props = withDefaults(defineProps<{
  uuid: string | null | undefined
  /** Pixel size for the avatar. Matches UserAvatar's vocabulary. */
  size?: 'xxs' | 'xs' | 'sm' | 'md'
  /** When true, only the avatar renders (no name text). The row
   * is still tab-stopped on the avatar so the user is reachable
   * by keyboard navigation. */
  avatarOnly?: boolean
  /** Whether clicking navigates to the user's profile route. */
  clickable?: boolean
}>(), {
  size: 'xxs',
  avatarOnly: false,
  clickable: false,
})

const { getUser } = useUsersDirectory()

const user = computed(() => (props.uuid ? getUser(props.uuid).value : null))

const userName = computed<string | undefined>(() => user.value?.name ?? undefined)

const avatarSrc = computed<string | null>(
  () => user.value?.avatar_thumb || user.value?.avatar_url || null,
)
</script>

<template>
  <div v-if="uuid" class="flex items-center gap-2 min-w-0">
    <UserAvatar
      :name="uuid"
      :user-name="userName"
      :avatar="avatarSrc"
      :size="size"
      :show-name="false"
      :clickable="clickable"
    />
    <span
      v-if="!avatarOnly"
      class="truncate text-[11px]"
      :class="userName ? 'text-secondary' : 'text-tertiary italic'"
    >
      {{ userName ?? 'Loading…' }}
    </span>
  </div>
  <span v-else class="text-xs text-tertiary italic">—</span>
</template>
