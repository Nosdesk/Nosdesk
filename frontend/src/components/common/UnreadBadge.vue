<script setup lang="ts">
/**
 * Small red unread-count badge. Used by the notification bell
 * trigger and the mobile bottom-nav Inbox tile. Returns nothing
 * when `count <= 0` so the parent can sit it next to an icon
 * without an empty-state placeholder.
 *
 * Positioning is the parent's job: apply `absolute -right-X -top-X`
 * (or whatever the surrounding layout needs) on the badge tag
 * itself, since the right offset varies per surface.
 */
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    count: number
    /** Cap displayed value; counts above render as "{cap}+". */
    cap?: number
  }>(),
  {
    cap: 99,
  },
)

const display = computed(() => {
  if (props.count <= 0) return null
  return props.count > props.cap ? `${props.cap}+` : String(props.count)
})
</script>

<template>
  <span
    v-if="display"
    class="inline-flex items-center justify-center min-w-[18px] h-[18px] px-1 rounded-full bg-status-error text-white text-xs font-bold leading-none"
    :aria-label="`${count} unread`"
  >
    {{ display }}
  </span>
</template>
