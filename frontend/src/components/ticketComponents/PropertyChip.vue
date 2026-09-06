<script setup lang="ts">
/**
 * PropertyChip — single chip for the property-list rows.
 *
 * Optional `to` makes the chip body a RouterLink; otherwise it's
 * a plain span. Optional `removable` surfaces a trailing X
 * button that emits `remove`. Optional leading slot lets
 * consumers prepend an emoji / icon / status dot.
 */
import { computed } from 'vue'
import type { RouteLocationRaw } from 'vue-router'
import { RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'

const props = defineProps<{
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

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const removeAriaLabel = computed(() => props.removeTitle || t('ticket-chip-remove', { label: props.label }))
</script>

<template>
  <component
    :is="to ? RouterLink : 'span'"
    :to="to"
    :title="title || label"
    class="print-chip inline-flex items-center gap-1 pl-2 pr-1 py-0.5 rounded text-2xs font-medium bg-surface-alt text-secondary hover:text-primary hover:bg-surface-hover transition-colors max-w-full"
    :class="{ 'opacity-60': loading }"
  >
    <slot name="leading" />
    <span class="truncate">{{ label }}</span>
    <button
      v-if="removable"
      type="button"
      class="inline-flex items-center justify-center w-4 h-4 rounded hover:bg-black/10 dark:hover:bg-white/10 transition-colors print:hidden"
      :title="removeAriaLabel"
      :aria-label="removeAriaLabel"
      @click.stop.prevent="emit('remove')"
    >
      <Icon name="close" class="w-3 h-3" />
    </button>
    <span v-else class="w-1" aria-hidden="true" />
  </component>
</template>

<style scoped>
/* In print the chip collapses to plain comma-separated text. Used by
   the ticket print sheet's "Referenced" section, where ProjectChip /
   LinkedTicketChip are reused purely to resolve names. On screen the
   chip is unchanged. */
@media print {
  .print-chip {
    display: inline !important;
    background: transparent !important;
    color: #222 !important;
    padding: 0 !important;
    border-radius: 0 !important;
    font-size: 9.5pt !important;
    font-weight: 400 !important;
    max-width: none !important;
  }

  .print-chip .truncate {
    overflow: visible;
    white-space: normal;
  }

  .print-chip:not(:last-child)::after {
    content: ", ";
    color: #888;
  }
}
</style>
