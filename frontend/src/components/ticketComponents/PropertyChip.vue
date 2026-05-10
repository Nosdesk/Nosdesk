<script setup lang="ts">
/**
 * PropertyChip — single chip for the property-list rows.
 *
 * Optional `to` makes the chip body a RouterLink; otherwise it's
 * a plain span. Optional `removable` surfaces a trailing X
 * button that emits `remove`. Optional leading slot lets
 * consumers prepend an emoji / icon / status dot.
 */
import type { RouteLocationRaw } from 'vue-router'
import { RouterLink } from 'vue-router'
import Icon from '@/components/common/Icon.vue'

defineProps<{
  label: string
  title?: string
  to?: RouteLocationRaw
  removable?: boolean
  removeTitle?: string
  /** Loading state - dims the chip while it resolves. */
  loading?: boolean
}>()

const emit = defineEmits<{
  (e: 'remove'): void
}>()
</script>

<template>
  <component
    :is="to ? RouterLink : 'span'"
    :to="to"
    :title="title || label"
    class="inline-flex items-center gap-1 pl-2 pr-1 py-0.5 rounded text-[11px] font-medium bg-surface-alt text-secondary hover:text-primary hover:bg-surface-hover transition-colors max-w-full"
    :class="{ 'opacity-60': loading }"
  >
    <slot name="leading" />
    <span class="truncate">{{ label }}</span>
    <button
      v-if="removable"
      type="button"
      class="inline-flex items-center justify-center w-4 h-4 rounded hover:bg-black/10 dark:hover:bg-white/10 transition-colors print:hidden"
      :title="removeTitle || `Remove ${label}`"
      :aria-label="removeTitle || `Remove ${label}`"
      @click.stop.prevent="emit('remove')"
    >
      <Icon name="close" class="w-3 h-3" />
    </button>
    <span v-else class="w-1" aria-hidden="true" />
  </component>
</template>
