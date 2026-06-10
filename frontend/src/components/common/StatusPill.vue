<script setup lang="ts">
/**
 * StatusPill: a small, reusable status chip with a tone-based palette.
 *
 * Replaces the hand-rolled conditional-Tailwind badges scattered across
 * the project and cycle views. Color is never the only signal: a leading
 * dot pairs with the tint and the label always carries the meaning, so it
 * stays legible in color-blind mode and at a glance.
 *
 * Tones map onto the semantic status tokens (see main.css):
 *   positive -> success/green, caution -> warning/amber,
 *   critical -> error/red, info -> blue, accent -> brand, neutral -> grey.
 */
import { computed } from 'vue'
import type { StatusPillTone } from './statusPillTone'

const props = withDefaults(
  defineProps<{
    label: string
    tone?: StatusPillTone
    size?: 'xs' | 'sm'
    /** Leading status dot (color-blind-safe pairing). On by default. */
    dot?: boolean
  }>(),
  { tone: 'neutral', size: 'xs', dot: true },
)

const toneClasses = computed(() => {
  switch (props.tone) {
    case 'positive':
      return 'bg-status-success-muted text-status-success border-status-success/30'
    case 'caution':
      return 'bg-status-warning-muted text-status-warning border-status-warning/30'
    case 'critical':
      return 'bg-status-error-muted text-status-error border-status-error/30'
    case 'info':
      return 'bg-status-info-muted text-status-info border-status-info/30'
    case 'accent':
      return 'bg-accent-muted text-accent border-accent/30'
    default:
      return 'bg-surface-hover text-tertiary border-default'
  }
})

const sizeClasses = computed(() =>
  props.size === 'sm'
    ? 'text-xs px-2 py-0.5 gap-1.5'
    : 'text-[10px] px-1.5 py-0.5 gap-1',
)
</script>

<template>
  <span
    class="inline-flex items-center rounded-full border font-semibold uppercase tracking-wide leading-none whitespace-nowrap"
    :class="[toneClasses, sizeClasses]"
  >
    <span
      v-if="dot"
      class="rounded-full bg-current shrink-0"
      :class="size === 'sm' ? 'w-1.5 h-1.5' : 'w-1 h-1'"
      aria-hidden="true"
    />
    {{ label }}
  </span>
</template>
