<script setup lang="ts">
/**
 * Action-menu items renderer. The shared chrome every popup
 * menu in the app uses: gutter-aligned icons (always-rendered
 * fixed-width column so labels line up across icon and label-
 * only items), dividers as preludes that can co-occur with any
 * item kind, section headings, checkmarks for radio-group items,
 * danger styling for destructive actions, accent styling for
 * "active" toggle states.
 *
 * The component is purely presentational: it takes a typed item
 * array and emits `select(id)`. Wrapping it in a popover is the
 * caller's job — `<ContextMenu>` does it for click-anchored
 * cases, `<DocumentActionsMenu>` does it for element-anchored
 * trigger buttons. This keeps the menu rendering identical
 * everywhere without coupling it to a specific anchoring model.
 */

export interface MenuItem {
  id: string
  label: string
  /** Raw SVG `path d=` string. Source from the central icon
   * registry (`ICON_REGISTRY.x.d`) so the same action carries
   * the same glyph app-wide. Mutually exclusive with `iconUrl`;
   * `icon` wins if both are set. */
  icon?: string
  /** Image URL or data URI rendered into the icon gutter as an
   * `<img>`. Use for runtime-supplied glyphs (e.g. plugin icons)
   * where the source isn't part of the central registry. */
  iconUrl?: string
  /** Right-aligned subtle text — keyboard shortcut, source
   * label, count badge, etc. Sits flush to the right edge of
   * the row, doesn't compete with the main label for space. */
  trailing?: string
  /** Destructive actions: red text + red hover background. */
  danger?: boolean
  /** Toggle "on" state (subscribed, starred, etc.): accent text. */
  active?: boolean
  /** Render a horizontal rule above this item. Composes with
   * heading and button items — divider is a prelude, not an
   * item-replacing kind. */
  divider?: boolean
  /** Render a checkmark in the icon gutter instead of `icon`.
   * Used for radio-group items so the active option reads
   * without a nested submenu. */
  checked?: boolean
  /** Render as a non-interactive section heading (small
   * uppercase label). Used to label inline groups. */
  heading?: boolean
}

defineProps<{ items: MenuItem[] }>()

const emit = defineEmits<{ select: [id: string] }>()
</script>

<template>
  <template v-for="item in items" :key="item.id">
    <!-- Divider is a prelude that can co-occur with any other
         item kind (heading, button). Rendered first so an item
         with `divider: true, heading: true` shows a separator
         above the section heading. -->
    <div v-if="item.divider" class="my-1 border-t border-subtle"></div>

    <!-- Section heading: non-interactive label for inline groups
         (e.g. "Sort by"). Mirrors the button's flex layout
         (icon gutter + label) so headings align vertically with
         the items underneath them. -->
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
          : item.active
            ? 'text-accent hover:text-accent-hover hover:bg-surface-hover'
            : 'text-secondary hover:text-primary hover:bg-surface-hover'
      "
      @click="emit('select', item.id)"
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
        <img
          v-else-if="item.iconUrl"
          :src="item.iconUrl"
          alt=""
          class="w-full h-full rounded-sm object-cover"
        />
      </span>
      <span class="flex-1 truncate">{{ item.label }}</span>
      <span v-if="item.trailing" class="ml-auto pl-2 text-[10px] text-tertiary flex-shrink-0">
        {{ item.trailing }}
      </span>
    </button>
  </template>
</template>
