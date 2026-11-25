<script setup lang="ts">
import { computed } from 'vue'
import Icon from '@/components/common/Icon.vue'
import type { IconName } from '@/components/common/icons'

export type CreateIconType = 'plus' | 'ticket' | 'user' | 'device' | 'project' | 'document'

const props = withDefaults(defineProps<{
  icon: CreateIconType
  loading?: boolean
}>(), {
  icon: 'plus',
  loading: false
})

// Create action icons: the domain glyph of the item being created
const iconNames: Record<CreateIconType, IconName> = {
  plus: 'add',
  ticket: 'ticket',
  user: 'user',
  device: 'device',
  project: 'project',
  document: 'document',
}

const currentIcon = computed(() => iconNames[props.icon] || iconNames.plus)
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
    <Icon v-else :name="currentIcon" size="md" />

    <!-- "Add" badge: small plus glyph anchored to the domain
         icon's top-right. Sized to feel like an indicator rather
         than a competing graphic, with a theme-aware surface bg
         + hairline ring so it reads as a separate element against
         either light or dark backgrounds (the prior hardcoded
         white bg punched through dark surfaces). -->
    <span
      v-if="showBadge"
      class="absolute -top-1 -right-1.5 size-3.5 flex items-center justify-center rounded-full bg-surface ring-1 ring-default"
      aria-hidden="true"
    >
      <svg class="size-2.5 text-accent-ui" viewBox="0 0 10 10" fill="none">
        <path d="M5 2.5v5M2.5 5h5" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
      </svg>
    </span>
  </span>
</template>
