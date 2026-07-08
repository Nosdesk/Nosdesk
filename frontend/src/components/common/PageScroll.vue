<script setup lang="ts">
/**
 * Page-level scroll layout primitive. Codifies the shape every
 * full-page list/feed view in the app needs:
 *
 *   ┌───────────────────────────────┐
 *   │ chrome (sticky, doesn't scroll)│
 *   ├───────────────────────────────┤
 *   │                               │
 *   │  scroll region                │
 *   │   ├─ default slot in a max-   │
 *   │   │  width readable column,   │
 *   │   │  OR                       │
 *   │   ├─ empty slot spanning the  │
 *   │   │  full scroll-container    │
 *   │   │  width and centred        │
 *   │                               │
 *   ├───────────────────────────────┤
 *   │ footer (sticky, doesn't scroll)│
 *   └───────────────────────────────┘
 *
 * The empty state spans the full scroll container width (not
 * the readable column) so the icon + copy centre against the
 * actual content area instead of being trapped offset inside a
 * narrow column. This was the bug `BaseListView` baked in by
 * forcing both the empty state and the list into the same flex
 * container; this primitive separates them.
 *
 * What this owns:
 *   - The root flex column + the inner scroll container
 *   - Switching between empty and default slot
 *   - The default content column's readable max-width (overridable)
 *
 * What this does NOT own:
 *   - Loading / error states (consumer choice — stick a banner
 *     in `#chrome`, render a skeleton inline in default slot,
 *     etc.)
 *   - The bulk-actions bar pattern (use `<BulkActionsBar>` inside
 *     `#chrome` when needed)
 *   - Any specific layout inside chrome / footer / empty / default
 *
 * Consumers needing the scroll container element (for an
 * `IntersectionObserver` root, scroll listeners, scroll-to-top,
 * etc.) read it off `defineExpose`:
 *
 *   const pageRef = ref<InstanceType<typeof PageScroll> | null>(null)
 *   // pageRef.value?.scrollContainerRef is the <div>
 */
import { ref } from 'vue'
import PullToRefresh from './PullToRefresh.vue'

// `inheritAttrs: false` so the parent `<RouterView>`'s class
// (`h-full overflow-auto`) doesn't merge with our root's
// `overflow-hidden` and rely on Tailwind utility-ordering
// to win the cascade. We own this layout end-to-end.
defineOptions({ inheritAttrs: false })

interface Props {
  /** Render the `empty` slot in place of the default content.
   * When true, the empty slot fills the scroll container at
   * full width and centres its content vertically. */
  isEmpty?: boolean
  /** Tailwind classes for the wrapper around the default slot.
   * Defaults to a centred max-w-5xl readable column with
   * responsive padding — right for the inbox, ticket lists,
   * and most feed-style views. Pass an empty string for
   * full-bleed content (e.g. a DataTable that owns its own
   * width). */
  contentClass?: string
  /** Pull-to-refresh action (Tauri app only). Defaults to the
   * global re-sync (pool delta + active-query refetch) — override
   * only when a view needs something more specific. */
  onRefresh?: () => Promise<unknown>
  /** Opt a view out of pull-to-refresh entirely. */
  noPullToRefresh?: boolean
}

withDefaults(defineProps<Props>(), {
  isEmpty: false,
  contentClass: 'mx-auto w-full max-w-5xl px-4 py-4 sm:px-6 sm:py-6 lg:px-8',
  onRefresh: undefined,
  noPullToRefresh: false,
})

// Typed as `HTMLDivElement` (not the more generic
// `HTMLElement`) so consumers feeding it to scroll APIs get
// div-specific autocomplete without a cast.
const scrollContainerRef = ref<HTMLDivElement | null>(null)

defineExpose({ scrollContainerRef })
</script>

<template>
  <div class="flex h-full flex-col overflow-hidden">
    <slot name="chrome" />

    <!-- The extra overflow-hidden wrapper clips the scroller while
         pull-to-refresh translates it, so pulled content can't slide
         over the footer. -->
    <div class="relative min-h-0 flex-1 overflow-hidden">
      <div ref="scrollContainerRef" class="h-full overflow-y-auto overscroll-y-contain">
        <!-- Empty state: full scroll-width, vertically centred.
             `h-full` claims the entire scroll viewport so the
             empty content sits visually in the middle. -->
        <div
          v-if="isEmpty"
          class="flex h-full flex-col items-center justify-center p-6"
        >
          <slot name="empty" />
        </div>

        <!-- Default content column. Width-constrained by default
             so list rows stay readable on ultrawide displays;
             pass `content-class=""` to opt out. -->
        <div v-else :class="contentClass">
          <slot />
        </div>
      </div>
    </div>

    <PullToRefresh
      :target="scrollContainerRef"
      :on-refresh="onRefresh"
      :disabled="noPullToRefresh"
    />

    <slot name="footer" />
  </div>
</template>
