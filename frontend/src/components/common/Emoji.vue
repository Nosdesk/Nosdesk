<script setup lang="ts">
/**
 * Emoji Component
 *
 * Renders emojis using Twemoji SVGs for consistent cross-platform display.
 * Supports theme-aware styling (grayscale for e-paper, amber for red-horizon).
 */
import { computed, ref, watch } from 'vue'
import { getEmojiUrl, isTwemojiPreloaded } from '@/composables/useTwemoji'

const props = withDefaults(defineProps<{
  emoji: string
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl' | 'inherit'
  alt?: string
  /** When true, skip lazy-loading so grid/picker icons paint immediately
   *  once their SVG is in the browser cache (see preloadTwemoji). */
  eager?: boolean
}>(), {
  size: 'md',
  eager: false,
})

const svgUrl = computed(() => getEmojiUrl(props.emoji))

const isReady = ref(isTwemojiPreloaded(props.emoji))

watch(
  () => props.emoji,
  (emoji) => {
    isReady.value = isTwemojiPreloaded(emoji)
  },
  { immediate: true },
)

function onLoad() {
  isReady.value = true
}

const sizeClass = computed(() => {
  switch (props.size) {
    case 'xs': return 'w-3 h-3'
    case 'sm': return 'w-4 h-4'
    case 'md': return 'w-5 h-5'
    case 'lg': return 'w-6 h-6'
    case 'xl': return 'w-8 h-8'
    case 'inherit': return 'w-[1em] h-[1em]'
    default: return 'w-5 h-5'
  }
})
</script>

<template>
  <img
    :src="svgUrl"
    :alt="alt || emoji"
    class="twemoji inline-block align-text-bottom transition-opacity duration-100"
    :class="[sizeClass, isReady ? 'opacity-100' : 'opacity-0']"
    draggable="false"
    :loading="eager ? 'eager' : 'lazy'"
    @load="onLoad"
  />
</template>
