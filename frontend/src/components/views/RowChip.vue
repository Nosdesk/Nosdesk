<script setup lang="ts">
/**
 * A chip on a phone's ticket row, sized by its own text: the padding,
 * the dot and the radius are in em, so it grows with its label and never
 * clips it. Sentence case at the 12px floor, which reads better than an
 * uppercase tracked pill and takes less width for the same word.
 *
 * The tone colours are the theme's text versions, which hold 4.5:1 on
 * their own muted fill (`themes/utils/readable.ts`); critical is the
 * overdue colour, as the due chip uses everywhere else.
 */
withDefaults(
  defineProps<{
    tone?: 'neutral' | 'critical' | 'caution'
    /** A leading dot, so the tone is never colour alone. */
    dot?: boolean
  }>(),
  { tone: 'neutral', dot: false },
)
</script>

<template>
  <span
    class="inline-flex items-center gap-[0.4em] rounded-full border px-[0.65em] py-[0.15em] text-2xs leading-[1.3] font-medium whitespace-nowrap tabular-nums"
    :class="{
      'border-default bg-surface-hover text-secondary': tone === 'neutral',
      'border-status-error/30 bg-status-error-muted text-overdue': tone === 'critical',
      'border-status-warning/30 bg-status-warning-muted text-status-warning': tone === 'caution',
    }"
  >
    <span v-if="dot" class="h-[0.45em] w-[0.45em] shrink-0 rounded-full bg-current" aria-hidden="true" />
    <slot />
  </span>
</template>
