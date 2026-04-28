<script setup lang="ts">
/**
 * Inline animated spinner. Use anywhere a button label, status
 * row, or async pill needs a "working on it" cue. The component
 * is intentionally small in scope, no layout, no text, no
 * centering. For the centered "loading the page" experience use
 * `LoadingSpinner.vue` (which composes this internally).
 *
 * Why not in `icons.ts`: a spinner is a two-layer SVG (faint
 * background ring + opaque arc) and needs `animate-spin`, so it
 * doesn't fit the registry's single-path stroke-or-fill model.
 * Carrying its own component keeps the registry honest as
 * "static action icons".
 *
 * Accessibility: renders `role="status"` and `aria-live="polite"`
 * with an SR-only label so the spinner is announced. Pass `label`
 * to override the default "Loading".
 */
import { computed } from 'vue'

interface Props {
  /** Tailwind size class applied to width + height. Defaults to
   * `h-4 w-4` so spinners match menu / toolbar sizing. */
  size?: 'xs' | 'sm' | 'md' | 'lg'
  /** Screen-reader label. Defaults to "Loading". */
  label?: string
}

const props = withDefaults(defineProps<Props>(), { size: 'sm' })

const sizeClass = computed(() => {
  switch (props.size) {
    case 'xs':
      return 'h-3 w-3'
    case 'md':
      return 'h-5 w-5'
    case 'lg':
      return 'h-8 w-8'
    case 'sm':
    default:
      return 'h-4 w-4'
  }
})
</script>

<template>
  <span role="status" aria-live="polite" class="inline-flex">
    <svg
      :class="['animate-spin text-current', sizeClass]"
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      aria-hidden="true"
    >
      <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
      <path
        class="opacity-75"
        fill="currentColor"
        d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
      />
    </svg>
    <span class="sr-only">{{ label ?? 'Loading' }}</span>
  </span>
</template>
