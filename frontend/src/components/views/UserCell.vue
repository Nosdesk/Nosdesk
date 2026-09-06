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
import { useFluent } from 'fluent-vue'
import UserAvatar from '@/components/UserAvatar.vue'
import { useUsersDirectory } from '@/composables/useUsersDirectory'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

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

const { getUserHandle } = useUsersDirectory()

const handle = computed(() => (props.uuid ? getUserHandle(props.uuid) : null))
const user = computed(() => handle.value?.user.value ?? null)
const status = computed(() => handle.value?.status.value ?? 'loading')

const userName = computed<string | undefined>(() => user.value?.name ?? undefined)

const avatarSrc = computed<string | null>(
  () => user.value?.avatar_thumb || user.value?.avatar_url || null,
)
</script>

<template>
  <div v-if="uuid" class="flex items-center gap-2 min-w-0">
    <UserAvatar
      :uuid="uuid"
      :fallbackName="userName"
      :fallbackAvatar="avatarSrc"
      :size="size"
      :show-name="false"
      :clickable="clickable"
    />
    <template v-if="!avatarOnly">
      <!-- Three rendering states keyed off the directory's status,
           not just the presence of a name. Without this split, a
           fetch that completes with "user not found" left the cell
           in skeleton forever because `userName` stayed undefined
           and the cell couldn't distinguish loading from missing. -->
      <span
        v-if="status === 'loading'"
        class="h-2.5 w-20 rounded bg-surface-alt animate-pulse shrink-0"
        aria-hidden="true"
      />
      <span
        v-else-if="status === 'resolved'"
        class="truncate text-2xs text-secondary"
      >{{ userName }}</span>
      <span
        v-else
        class="text-2xs text-tertiary italic shrink-0"
        :title="t('user-cell-missing-tooltip')"
      >{{ t('user-cell-unknown') }}</span>
    </template>
  </div>
  <span v-else class="text-xs text-tertiary italic">-</span>
</template>
