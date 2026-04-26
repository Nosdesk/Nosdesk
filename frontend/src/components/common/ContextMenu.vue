<script setup lang="ts">
/**
 * Right-click / "more actions" menu. Public API unchanged from
 * the standalone implementation; positioning, dismiss, focus,
 * and viewport handling all delegate to `<Popover>` so this
 * component can stay focused on the menu's interaction model
 * (item selection, headings, dividers, danger states).
 */
import { computed } from 'vue'
import Popover from './Popover.vue'

export interface MenuItem {
  id: string
  label: string
  icon?: string
  danger?: boolean
  divider?: boolean
  /** When true, renders a checkmark in the icon gutter instead
   * of `icon`. Use for radio-group items inside the menu (e.g.
   * a sort-mode picker) so the active option is visible without
   * a nested submenu. */
  checked?: boolean
  /** When true, renders as a non-interactive section heading
   * (small uppercase label). Used to label inline groups like
   * "Sort by" sitting above their options. */
  heading?: boolean
}

const props = defineProps<{
  items: MenuItem[]
  x: number
  y: number
}>()

const emit = defineEmits<{
  select: [id: string]
  close: []
}>()

const handleSelect = (id: string) => {
  emit('select', id)
  emit('close')
}

// Anchor the menu to the click point. Re-derived from props so
// changes to x/y after open (e.g. a re-trigger on a different
// row) reposition without a full unmount.
const anchor = computed(() => ({
  type: 'point' as const,
  x: props.x,
  y: props.y,
}))
</script>

<template>
  <Popover
    :open="true"
    :anchor="anchor"
    placement="bottom-start"
    react-to-scroll="close"
    role="menu"
    popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[160px]"
    @close="emit('close')"
  >
    <template v-for="item in items" :key="item.id">
      <!-- Divider is a prelude that can co-occur with any other
           item kind (heading, button). Rendered first so an item
           with `divider: true, heading: true` shows a separator
           above the section heading. -->
      <div v-if="item.divider" class="my-1 border-t border-subtle"></div>
      <!-- Section heading: non-interactive label for inline
           groups (e.g. "Sort by"). Mirrors the button's flex
           layout (icon gutter + label) so headings align
           vertically with the items underneath them. -->
      <div
        v-if="item.heading"
        class="w-full px-3 pt-2 pb-1 flex items-center gap-2 text-[10px] font-semibold tracking-wide text-tertiary uppercase select-none"
      >
        <span class="w-3.5 h-3.5 flex-shrink-0" aria-hidden="true"></span>
        <span>{{ item.label }}</span>
      </div>
      <button
        v-else
        role="menuitem"
        class="w-full px-3 py-1.5 text-xs text-left flex items-center gap-2 transition-colors"
        :class="
          item.danger
            ? 'text-red-500 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-950/30'
            : 'text-secondary hover:text-primary hover:bg-surface-hover'
        "
        @click="handleSelect(item.id)"
      >
        <!-- Always-rendered icon gutter. Reserves the same width
             whether or not this item has an icon, so labels align
             down the column even in mixed menus. The same gutter
             doubles as the active-state indicator: when `checked`
             is true the icon swaps for a check glyph, so a
             radio-group reads as "the one with the tick is
             current" without nesting or a separate column. -->
        <span class="w-3.5 h-3.5 flex-shrink-0 flex items-center justify-center">
          <svg
            v-if="item.checked"
            class="w-full h-full text-accent"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            stroke-width="2.5"
          >
            <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
          </svg>
          <svg
            v-else-if="item.icon"
            class="w-full h-full"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            stroke-width="2"
          >
            <path stroke-linecap="round" stroke-linejoin="round" :d="item.icon" />
          </svg>
        </span>
        <span>{{ item.label }}</span>
      </button>
    </template>
  </Popover>
</template>
