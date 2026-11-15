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
import { ICON_REGISTRY, type IconName, type IconDef } from './icons'

interface Props {
  name: IconName
  /** Tailwind size class applied to width + height. Defaults to
   * `h-4 w-4` (`sm`) so icons match menu / toolbar sizing.
   *
   *   xs  h-3   12px  inline badges, chips
   *   sm  h-4   16px  menus, toolbars, table cells
   *   md  h-5   20px  buttons, list rows
   *   lg  h-6   24px  headers, nav
   *   xl  h-8   32px  card and dropzone glyphs
   *   2xl h-12  48px  empty-state and not-found hero glyphs
   *
   * Two large steps cover every hero site in the app (w-8, w-10 and
   * w-12 dominate; the odd w-16 rounds down to `2xl`). */
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl' | '2xl'
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
    case 'xl':
      return 'h-8 w-8'
    case '2xl':
      return 'h-12 w-12'
    case 'sm':
    default:
      return 'h-4 w-4'
  }
})

/** Stroke width in viewBox units. 2 everywhere, with one exception:
 * at `2xl` a 2-unit stroke is a 4px line, heavier than the text it
 * sits over, and every hero glyph the app shipped before the
 * registry drew at 1.5 (a 3px line at 48px). Keeping that weight
 * means the swap to `<Icon>` is invisible at that size. */
const strokeWidth = computed(() => (props.size === '2xl' ? 1.5 : 2))

const def = computed<IconDef>(() => ICON_REGISTRY[props.name])
</script>

<template>
  <svg
    xmlns="http://www.w3.org/2000/svg"
    :class="sizeClass"
    viewBox="0 0 24 24"
    :fill="def.filled ? 'currentColor' : 'none'"
    :stroke="def.filled ? 'none' : 'currentColor'"
    :stroke-width="strokeWidth"
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
