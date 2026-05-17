<script setup lang="ts">
/**
 * Avatar stack rendering an active viewer set.
 *
 * Generic over `viewers` shape: the parent decides ordering, who
 * to include, and how to filter (e.g. "self excluded" for ticket
 * detail, or "all" for the future cross-ticket presence view).
 * This component is purely a presentational stack with overlap
 * styling, a "+N" overflow pip, and TransitionGroup join/leave
 * animations.
 *
 * Avatar fill is resolved here via `useUsersDirectory` so the
 * parent only has to forward uuids. The directory's pool-backed
 * lookup is in-memory after bootstrap, so a list of 50 viewers
 * still resolves synchronously on render.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import type { FluentVariable } from '@fluent/bundle'
import UserAvatar from '@/components/UserAvatar.vue'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import type { ViewerInfo } from '@/types/sse'

const fluent = useFluent()
const t = (k: string, args?: Record<string, FluentVariable>) => fluent.$t(k, args)

interface Props {
  viewers: ViewerInfo[]
  /** Cap on the number of avatars rendered before collapsing the
   * remainder into a "+N" pip. Default tuned for the ticket header
   * row; cross-ticket views may want a wider cap. */
  maxVisible?: number
  /** Avatar pixel size. Defaults to `sm` to fit the header strip. */
  size?: 'xxs' | 'xs' | 'sm' | 'md'
}

const props = withDefaults(defineProps<Props>(), {
  maxVisible: 3,
  size: 'sm',
})

const { getUser } = useUsersDirectory()

/**
 * Pre-resolve `{ uuid, name }` once per render rather than calling
 * `getUser` twice per viewer (once for the wrapper tooltip, once
 * for the avatar's userName prop). Vue's reactivity tracks the
 * underlying pool through the `getUser(...).value` access here, so
 * the computed re-runs when a user's name updates mid-session.
 */
const viewerCards = computed(() =>
  props.viewers.slice(0, props.maxVisible).map((v) => ({
    uuid: v.user_uuid,
    name: getUser(v.user_uuid).value?.name ?? t('ui-presence-stack-fallback-name'),
  }))
)

const extraCount = computed(() =>
  Math.max(0, props.viewers.length - props.maxVisible)
)

const ariaLabel = computed(() => {
  const n = props.viewers.length
  if (n === 0) return ''
  return t('ui-presence-stack-aria', { count: n })
})

const overflowTitle = computed(() =>
  t('ui-presence-stack-overflow-title', { count: extraCount.value })
)
</script>

<template>
  <!-- TransitionGroup as the root: avatar overlap (negative
       margin) only works when the avatars are direct children of
       the flex container, so the transition wrapper IS the flex
       container, not a nested element. Soft fade-scale on join /
       leave keeps a single-tab close from looking like a glitch.
       v-if keeps an empty stack from taking flex-gap space; the
       trade-off is that the *very last* viewer's leave is a snap
       rather than a fade, which is the rare case nobody watches. -->
  <TransitionGroup
    v-if="viewers.length > 0"
    name="presence"
    tag="div"
    class="flex items-center -space-x-2"
    :aria-label="ariaLabel"
  >
    <div
      v-for="card in viewerCards"
      :key="card.uuid"
      class="ring-2 ring-surface rounded-full"
    >
      <UserAvatar
        :uuid="card.uuid"
        :fallbackName="card.name"
        :show-name="false"
        :clickable="false"
        :size="size"
      />
    </div>
    <div
      v-if="extraCount > 0"
      key="overflow"
      class="h-6 w-6 rounded-full bg-surface-alt ring-2 ring-surface flex items-center justify-center text-[0.625rem] font-medium text-secondary"
      :title="overflowTitle"
    >
      +{{ extraCount }}
    </div>
  </TransitionGroup>
</template>

<style scoped>
.presence-enter-active,
.presence-leave-active {
  transition: opacity 180ms ease-out, transform 180ms ease-out;
}
.presence-enter-from,
.presence-leave-to {
  opacity: 0;
  transform: scale(0.7);
}

@media (prefers-reduced-motion: reduce) {
  .presence-enter-active,
  .presence-leave-active {
    transition: opacity 80ms linear;
    transform: none;
  }
}
</style>
