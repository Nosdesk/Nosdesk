<script setup lang="ts">
import { computed } from 'vue'

export type CreateIconType = 'plus' | 'ticket' | 'user' | 'device' | 'project' | 'document'

const props = withDefaults(defineProps<{
  icon: CreateIconType
  loading?: boolean
}>(), {
  icon: 'plus',
  loading: false
})

// Create action icons - represent creating a new item
const iconPaths: Record<CreateIconType, string> = {
  plus: 'M12 4.5v15m7.5-7.5h-15',
  ticket: 'M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9h6m-6-4h6',
  user: 'M15.75 6a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0zM4.501 20.118a7.5 7.5 0 0114.998 0',
  device: 'M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z',
  project: 'M4 4h4v16H4V4zm6 0h4v12h-4V4zm6 0h4v8h-4V4z',
  document: 'M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z'
}

const currentPath = computed(() => iconPaths[props.icon] || iconPaths.plus)
const showBadge = computed(() => props.icon !== 'plus' && !props.loading)
</script>

<template>
  <span class="relative inline-flex">
    <!-- Loading spinner -->
    <svg v-if="loading" class="w-5 h-5 animate-spin" fill="none" viewBox="0 0 24 24">
      <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
      <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
    </svg>

    <!-- Main icon -->
    <svg v-else class="w-5 h-5" fill="none" stroke="currentColor" stroke-width="1.5" viewBox="0 0 24 24">
      <path stroke-linecap="round" stroke-linejoin="round" :d="currentPath" />
    </svg>

    <!-- Plus badge -->
    <span
      v-if="showBadge"
      class="absolute -top-1 -right-1.5 size-3 flex items-center justify-center rounded-full bg-white shadow-sm"
    >
      <svg class="size-2 text-accent" viewBox="0 0 10 10" fill="none">
        <path d="M5 2v6M2 5h6" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
      </svg>
    </span>
  </span>
</template>
