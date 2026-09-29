<script setup lang="ts">
/**
 * A person's avatar in the portal: their photo, or their initials on the same
 * colour the agent app gives them, so the team looks the same on both sides.
 */
import { computed, ref } from 'vue'

import { initialsFrom } from '@/utils/monogram'

const props = withDefaults(
  defineProps<{ name: string; src?: string | null; size?: 'sm' | 'md' | 'lg' }>(),
  { src: null, size: 'md' },
)

const failed = ref(false)
const sizeClass = computed(
  () => ({ sm: 'w-6 h-6 text-[0.625rem]', md: 'w-8 h-8 text-xs', lg: 'w-10 h-10 text-sm' })[props.size],
)
// UserAvatar's hue: from the first letter.
const color = computed(() => {
  const letter = props.name.charAt(0).toUpperCase()
  const hue = Math.abs((letter.charCodeAt(0) - 65) % 26) * (360 / 26)
  return `hsl(${hue}, 70%, 35%)`
})
</script>

<template>
  <img
    v-if="src && !failed"
    :src="src"
    alt=""
    class="rounded-full object-cover shrink-0"
    :class="sizeClass"
    loading="lazy"
    @error="failed = true"
  />
  <span
    v-else
    class="rounded-full shrink-0 inline-flex items-center justify-center font-medium text-white select-none"
    :class="sizeClass"
    :style="{ backgroundColor: color }"
    aria-hidden="true"
  >
    {{ initialsFrom(name) }}
  </span>
</template>
