<script setup lang="ts">
/**
 * Emoji Component
 *
 * Renders emojis using Twemoji SVGs for consistent cross-platform display.
 * Supports theme-aware styling (grayscale for e-paper, amber for red-horizon).
 */
import { computed } from 'vue'
import twemoji from '@twemoji/api'

const props = withDefaults(defineProps<{
  emoji: string
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl' | 'inherit'
  alt?: string
}>(), {
  size: 'md'
})

// Convert emoji to Twemoji SVG URL
const svgUrl = computed(() => {
  // Handle multi-codepoint emojis (flags, skin tones, ZWJ sequences)
  const codepoints = twemoji.convert.toCodePoint(props.emoji)
  return `https://cdn.jsdelivr.net/gh/jdecked/twemoji@latest/assets/svg/${codepoints}.svg`
})

// Size classes
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
    class="twemoji inline-block align-text-bottom"
    :class="sizeClass"
    draggable="false"
    loading="lazy"
  />
</template>
