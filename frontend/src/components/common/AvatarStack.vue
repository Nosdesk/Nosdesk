<script setup lang="ts">
/**
 * Overlapping stack of user avatars with a "+N" overflow chip.
 * Avatars are non-clickable and name-less, the chrome around the
 * stack owns the click target. Each avatar gets a ring in the
 * surface colour so the overlap reads as separate discs.
 */
import { computed } from 'vue'
import UserAvatar from '@/components/UserAvatar.vue'

type Size = 'xxs' | 'xs' | 'sm'

const props = withDefaults(
  defineProps<{
    uuids: string[]
    max?: number
    size?: Size
  }>(),
  { max: 4, size: 'xs' },
)

const shown = computed(() => props.uuids.slice(0, props.max))
const overflow = computed(() => Math.max(0, props.uuids.length - props.max))

const chipSize: Record<Size, string> = {
  xxs: 'h-4 w-4 text-[0.5rem]',
  xs: 'h-5 w-5 text-[0.5625rem]',
  sm: 'h-6 w-6 text-xs',
}
</script>

<template>
  <div v-if="uuids.length > 0" class="flex items-center -space-x-1.5">
    <div v-for="uuid in shown" :key="uuid" class="rounded-full ring-2 ring-surface">
      <UserAvatar :uuid="uuid" :size="size" :show-name="false" :clickable="false" />
    </div>
    <div
      v-if="overflow > 0"
      class="inline-flex items-center justify-center rounded-full ring-2 ring-surface bg-surface-alt text-tertiary font-medium"
      :class="chipSize[size]"
    >
      +{{ overflow }}
    </div>
  </div>
</template>
