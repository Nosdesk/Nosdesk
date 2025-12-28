<script setup lang="ts">
import { computed } from 'vue'

export type CreateIconType = 'plus' | 'ticket' | 'user' | 'device' | 'folder' | 'document'

const props = withDefaults(defineProps<{
  icon: CreateIconType
  loading?: boolean
  size?: 'sm' | 'md'
}>(), {
  icon: 'plus',
  loading: false,
  size: 'md'
})

// Icon SVG paths for each type
const iconPaths: Record<CreateIconType, string> = {
  plus: 'M12 6v6m0 0v6m0-6h6m-6 0H6',
  ticket: 'M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2',
  user: 'M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z',
  device: 'M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z',
  folder: 'M4 4h4v16H4V4zm6 0h4v12h-4V4zm6 0h4v8h-4V4z',
  document: 'M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z'
}

const currentPath = computed(() => iconPaths[props.icon] || iconPaths.plus)
const showPlusBadge = computed(() => props.icon !== 'plus' && !props.loading)

const iconSize = computed(() => props.size === 'sm' ? 'w-4 h-4' : 'w-5 h-5')
const badgeSize = computed(() => props.size === 'sm' ? 'w-3 h-3' : 'w-3.5 h-3.5')
const badgeIconSize = computed(() => props.size === 'sm' ? 'w-2 h-2' : 'w-2.5 h-2.5')
</script>

<template>
  <span class="relative inline-flex items-center justify-center">
    <!-- Loading spinner -->
    <svg
      v-if="loading"
      :class="[iconSize, 'animate-spin']"
      fill="none"
      viewBox="0 0 24 24"
    >
      <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
      <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
    </svg>

    <!-- Main icon -->
    <svg
      v-else
      :class="iconSize"
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" :d="currentPath" />
    </svg>

    <!-- Plus badge (only for non-plus icons) - positioned outside icon bounds -->
    <span
      v-if="showPlusBadge"
      :class="[badgeSize, 'absolute -top-1.5 -right-1.5 bg-surface text-accent rounded-full flex items-center justify-center shadow-sm border border-default']"
    >
      <svg :class="badgeIconSize" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="3">
        <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v12m6-6H6" />
      </svg>
    </span>
  </span>
</template>
