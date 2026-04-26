<script setup lang="ts">
/**
 * Single icon renderer. Looks up the path from `icons.ts` and
 * emits a clean SVG. Any reusable action icon in the app should
 * route through here so:
 *
 *   - One action -> one icon, app-wide
 *   - Stroke width / aria treatment / size scale stay consistent
 *   - The icon registry stays the single source of truth
 *
 * For decorative inline glyphs that are tightly coupled to a
 * specific layout (custom drop indicators, brand marks), inline
 * SVG is still fine — the registry is for shared *action* icons.
 */
import { computed } from 'vue'
import { ICON_REGISTRY, type IconName } from './icons'

interface Props {
  name: IconName
  /** Tailwind size class applied to width + height. Defaults to
   * `h-4 w-4` so icons match menu / toolbar sizing. */
  size?: 'xs' | 'sm' | 'md' | 'lg'
  /** When provided, renders an aria-label and `role="img"` so
   * the icon is announced by screen readers. Omit for purely
   * decorative icons (default — `aria-hidden="true"`). */
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
      return 'h-6 w-6'
    case 'sm':
    default:
      return 'h-4 w-4'
  }
})

const def = computed(() => ICON_REGISTRY[props.name])
</script>

<template>
  <svg
    xmlns="http://www.w3.org/2000/svg"
    :class="sizeClass"
    viewBox="0 0 24 24"
    :fill="def.filled ? 'currentColor' : 'none'"
    :stroke="def.filled ? 'none' : 'currentColor'"
    stroke-width="2"
    :aria-hidden="label ? undefined : true"
    :aria-label="label"
    :role="label ? 'img' : undefined"
  >
    <path
      v-if="!def.filled"
      stroke-linecap="round"
      stroke-linejoin="round"
      :d="def.d"
    />
    <path v-else :d="def.d" />
  </svg>
</template>
