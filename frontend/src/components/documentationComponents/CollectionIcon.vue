<script setup lang="ts">
import { computed } from 'vue'
import Emoji from '@/components/common/Emoji.vue'
import { collectionIconBackgroundStyle } from '@nosdesk/core/utils/collectionIconStyle'

const props = withDefaults(defineProps<{
  icon?: string | null
  color?: string | null
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl'
}>(), {
  icon: '📁',
  size: 'sm',
})

const containerClass = computed(() => {
  switch (props.size) {
    case 'xs': return 'w-5 h-5 rounded-md'
    case 'md': return 'w-8 h-8 rounded-lg'
    case 'lg': return 'w-10 h-10 rounded-lg'
    case 'xl': return 'w-16 h-16 rounded-xl'
    default: return 'w-6 h-6 rounded-md'
  }
})

const emojiSize = computed((): 'sm' | 'md' | 'lg' | 'xl' => {
  switch (props.size) {
    case 'xs': return 'sm'
    case 'md': return 'lg'
    case 'lg': return 'xl'
    case 'xl': return 'xl'
    default: return 'md'
  }
})

const backgroundStyle = computed(() => collectionIconBackgroundStyle(props.color))
</script>

<template>
  <span
    class="inline-flex shrink-0 items-center justify-center"
    :class="containerClass"
    :style="backgroundStyle"
    aria-hidden="true"
  >
    <Emoji :emoji="icon || '📁'" :size="emojiSize" />
  </span>
</template>
