<script setup lang="ts">
/**
 * SectionCard. The single canonical "card with a header" chrome.
 * Mirrors the dashboard widget shell so every card-with-header in
 * the app shares the same visual vocabulary: rounded surface, fixed
 * compact header pill, configurable body padding.
 *
 * Header anatomy (px-3, h-9, bg-surface-alt with border-b):
 *   [optional leading slot] [title] [headerActions slot] [actionTo link]
 *
 * For more elaborate state machines (loading skeleton, empty / error
 * states, edit-mode affordances), see `DashboardWidgetShell.vue`,
 * which is purpose-built for grid widgets and composes the same
 * header look on top.
 */

interface Props {
  /** Right-aligned action link in the header. Omit when the card
   *  has no drill-down. */
  actionTo?: string
  /** Label for the action link. Defaults to "View all". */
  actionLabel?: string
  /** Tailwind padding class applied to the body. Defaults to `p-3`;
   *  pass an empty string for flush-edge content (lists, tables). */
  contentPadding?: string
  /** When false, omits `overflow-hidden` so `position: sticky`
   *  descendants can stick to an outer scroll container. Default
   *  true — most cards rely on clipping for rounded corners. */
  clipContent?: boolean
  /** Pin the header while an ancestor scroll container moves. Pair
   *  with `clip-content="false"` when the scrollport is outside
   *  this card (kanban swimlanes scrolling vertically). */
  stickyHeader?: boolean
}

withDefaults(defineProps<Props>(), {
  actionLabel: 'View all',
  contentPadding: 'p-3',
  clipContent: true,
  stickyHeader: false,
})
</script>

<template>
  <div
    class="bg-surface rounded-xl border border-default flex flex-col"
    :class="{ 'overflow-hidden': clipContent }"
  >
    <!-- Header. Fixed-height compact pill, mirrors the dashboard
         widget shell so every card with a header reads as the same
         visual primitive across the app. -->
    <header
      class="flex items-center gap-2 px-3 h-9 border-b border-default bg-surface-alt flex-shrink-0"
      :class="{ 'sticky top-0 z-20': stickyHeader }"
    >
      <!-- Optional leading content (icon, dot indicator). -->
      <slot name="leading" />

      <h2 class="text-[13px] font-semibold text-primary truncate flex-1 tracking-tight">
        <slot name="title" />
      </h2>

      <!-- Card-specific header controls (filter dropdowns, toggles). -->
      <slot name="headerActions" />

      <router-link
        v-if="actionTo"
        :to="actionTo"
        class="text-[11px] font-medium text-accent hover:underline whitespace-nowrap"
      >
        {{ actionLabel }} →
      </router-link>
    </header>

    <div :class="contentPadding">
      <slot />
    </div>
  </div>
</template>

<style scoped>
@media print {
  /* Strip card chrome for print — the heading carries the section
   * label, the body becomes the printed content. */
  .bg-surface-alt {
    background: transparent !important;
    padding: 0 0 4pt 0 !important;
    border-bottom: 1px solid #ccc !important;
    margin-bottom: 6pt;
  }

  .bg-surface-alt :deep(h2) {
    font-size: 10pt !important;
    font-weight: 600 !important;
    margin: 0 !important;
  }

  .bg-surface {
    background: transparent !important;
    border: none !important;
    border-radius: 0 !important;
  }
}
</style>
