<script setup lang="ts">
/**
 * Viewport-aware floating menu surface. At `md` and above it
 * renders as a `<Popover>` (anchored, fade-scale, click-outside
 * dismiss). Below `md` it renders as a bottom sheet — the
 * touch-native pattern for "transient surface that takes part
 * of the screen, dismissable by drag/backdrop." Both share the
 * same slot, so consumers write the menu content once.
 *
 * Why a wrapper instead of a `bottomSheet` prop on `<Popover>`:
 * popover and bottom-sheet are different interaction models
 * (anchored vs modal-ish, click-outside vs drag-down). Mature
 * design systems (Radix, Headless UI) keep them as separate
 * primitives and let consumers compose. This wrapper IS the
 * compose step — so the Popover primitive stays single-purpose
 * while consumers get one component to call.
 *
 * Drag math, breakpoint reactivity, and dismiss-on-cross are
 * shared with `<ResponsivePanel>` via `useResponsiveSheet`.
 */
import { computed, toRef } from 'vue'
import Popover from './Popover.vue'
import { useScrollLock } from '@/composables/useScrollLock'
import { useResponsiveSheet } from '@/composables/useResponsiveSheet'
import type { PopoverAnchor, PopoverPlacement } from '@/composables/usePopover'

interface Props {
  open: boolean
  /** Anchor for the desktop popover layout. Ignored on mobile
   * (sheets are screen-anchored, not element-anchored). */
  anchor: PopoverAnchor
  /** Optional sheet header label. Bottom sheets read better
   * with a title; popovers usually don't need one (the trigger
   * provides the context). */
  title?: string
  // ---- Pass-through to Popover (desktop) -------------------
  placement?: PopoverPlacement
  reactToScroll?: 'close' | 'reposition'
  matchAnchorWidth?: boolean
  minWidth?: number
  offset?: number
  /** Move keyboard focus to the popover root on open. Right
   * default for action menus; dropdowns that want to keep
   * focus on the trigger should pass `false`. */
  autoFocus?: boolean
  role?: 'menu' | 'listbox' | 'dialog'
  ariaLabel?: string
  /** Tailwind chrome for the popover surface. The bottom sheet
   * has its own chrome (rounded top, shadow, drag handle); this
   * class only applies on desktop. */
  popoverClass?: string
}

const props = withDefaults(defineProps<Props>(), {
  placement: 'bottom-start',
  reactToScroll: 'reposition',
  matchAnchorWidth: false,
  offset: 4,
  autoFocus: true,
  role: 'menu',
})

const emit = defineEmits<{ (e: 'close'): void }>()

const { isMobile, dragOffset, isDragging, handleListeners } = useResponsiveSheet({
  open: toRef(props, 'open'),
  onDismiss: () => emit('close'),
})

// Lock body scroll only while the bottom sheet is open. Desktop
// popovers don't need it — the document keeps its own
// scrollable space.
const shouldLockBodyScroll = computed(() => props.open && isMobile.value)
useScrollLock(shouldLockBodyScroll)

const ariaLabel = computed(() => props.ariaLabel ?? props.title)
</script>

<template>
  <!-- Desktop: render the existing Popover with all positioning
       props passed through. Single-purpose primitive doing what
       it's good at. -->
  <Popover
    v-if="!isMobile"
    :open="open"
    :anchor="anchor"
    :placement="placement"
    :react-to-scroll="reactToScroll"
    :match-anchor-width="matchAnchorWidth"
    :min-width="minWidth"
    :offset="offset"
    :auto-focus="autoFocus"
    :role="role"
    :aria-label="ariaLabel"
    :popover-class="popoverClass"
    @close="emit('close')"
  >
    <slot />
  </Popover>

  <!-- Mobile: bottom sheet. Same slot content, different
       chrome. Backdrop tap dismisses; drag the handle past the
       threshold dismisses; otherwise springs back. -->
  <Teleport to="body">
    <Transition name="sheet-backdrop">
      <div
        v-if="open && isMobile"
        class="fixed inset-0 z-backdrop bg-black/40"
        aria-hidden="true"
        @click="emit('close')"
      />
    </Transition>
    <Transition name="sheet">
      <div v-if="open && isMobile" class="fixed inset-x-0 bottom-0 z-overlay">
        <!-- Inner panel: the Vue transition slides the WRAPPER on enter/leave,
             this panel carries the drag offset. Two elements so the two transforms
             compose, instead of the inline drag transform overriding (and killing)
             the slide-in. -->
        <div
          class="flex max-h-[80vh] flex-col rounded-t-xl border-t border-default bg-surface shadow-2xl"
          :class="{ 'sheet-panel-settle': !isDragging }"
          :style="{ transform: `translateY(${dragOffset}px)` }"
          :role="role"
          :aria-label="ariaLabel"
        >
        <!-- Drag handle pill. Tappable area extends beyond the
             pill so the user has more thumb room. -->
        <div
          class="flex flex-shrink-0 cursor-grab items-center justify-center pt-2 pb-1 select-none active:cursor-grabbing"
          v-on="handleListeners"
        >
          <div class="h-1 w-10 rounded-full bg-border"></div>
        </div>
        <h2
          v-if="title"
          class="flex-shrink-0 px-4 pt-1 pb-3 text-sm font-semibold text-primary"
        >
          {{ title }}
        </h2>
        <!-- Bottom padding clears the iPhone home indicator so the last option
             isn't in the dead zone; falls back to 0.5rem where there's no inset. -->
        <div class="flex flex-1 flex-col overflow-y-auto pb-[calc(0.5rem+env(safe-area-inset-bottom))]">
          <slot />
        </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
