<script setup lang="ts">
/**
 * Generic floating-element primitive, on Reka UI's Popover.
 *
 * Reka owns positioning (floating-ui: flip, shift, collision padding,
 * anchor tracking through scroll and resize), the dismiss layer stack
 * (Escape and outside-pointer only reach the topmost layer, so a
 * DatePicker opened from inside a menu never dismisses the menu, and a
 * popover inside a dialog closes before the dialog), the portal and the
 * `data-state` this file animates from.
 *
 * The public API is unchanged. Consumers pass the anchor rather than
 * rendering a `PopoverTrigger`: an `element` anchor becomes the floating
 * reference directly, a `point` anchor (right-click menus) becomes a
 * floating-ui virtual element at that viewport coordinate. Because the
 * anchor is not a Reka trigger, a pointerdown on it is filtered out of
 * outside-dismiss here, so a toggle button stays a toggle.
 *
 * Two scroll strategies, picked per use case:
 *   - `'reposition'`: follow the anchor (trigger-anchored dropdowns)
 *   - `'close'`: dismiss (click-anchored menus, where the point is stale)
 */
import { computed } from 'vue'
import { PopoverContent, PopoverPortal, PopoverRoot } from 'reka-ui'
import {
  anchorElementOf,
  floatingFrom,
  restoreFocusTo,
  type PopoverAnchor,
  type PopoverPlacement,
} from '@/composables/popoverAnchor'
import { useEventListener } from '@/composables/useEventListener'

interface Props {
  /** Open state. Parent owns it. */
  open: boolean
  /** What we anchor against. Reactively re-read on each update. */
  anchor: PopoverAnchor
  /** Preferred placement; flips when it does not fit. */
  placement?: PopoverPlacement
  reactToScroll?: 'close' | 'reposition'
  matchAnchorWidth?: boolean
  minWidth?: number
  offset?: number
  role?: 'menu' | 'listbox' | 'dialog' | 'tooltip'
  ariaLabel?: string
  popoverClass?: string
  autoFocus?: boolean
  noTransition?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  placement: 'bottom-start',
  reactToScroll: 'reposition',
  matchAnchorWidth: false,
  offset: 4,
  role: 'menu',
  autoFocus: true,
  noTransition: false,
})

const emit = defineEmits<{ (e: 'close'): void }>()

const floating = computed(() => floatingFrom(props.anchor, props.placement))
const side = computed(() => floating.value.side)
const align = computed(() => floating.value.align)
const reference = computed(() => floating.value.reference)
const anchorElement = () => anchorElementOf(props.anchor)

function onOpenChange(open: boolean) {
  if (!open) emit('close')
}

// The anchor is not a Reka trigger, so tell the dismiss layer a
// pointerdown on it is not "outside": the consumer's own click handler
// toggles, and closing here first would make every toggle re-open. A
// genuine outside pointerdown is remembered so close does not pull
// focus back from wherever the user just clicked.
let closedByOutsidePointer = false
function onPointerDownOutside(event: CustomEvent<{ originalEvent: PointerEvent }>) {
  const target = event.detail.originalEvent.target as Node | null
  const el = anchorElement()
  if (target && el?.contains(target)) event.preventDefault()
  else closedByOutsidePointer = true
}

function onOpenAutoFocus(event: Event) {
  closedByOutsidePointer = false
  if (!props.autoFocus) event.preventDefault()
}

// Reka restores focus to its trigger; ours is the anchor element. Done
// for keyboard closes (Escape, a pick) whether or not the surface took
// focus on open: a filter input inside it may have, and focus must not
// fall to the body.
function onCloseAutoFocus(event: Event) {
  event.preventDefault()
  if (!closedByOutsidePointer) restoreFocusTo(props.anchor)
  closedByOutsidePointer = false
}

// Point anchors go stale on scroll; element anchors are tracked by
// floating-ui, so nothing to do for them.
useEventListener(
  window,
  'scroll',
  () => {
    if (props.reactToScroll === 'close') emit('close')
  },
  { when: computed(() => props.open), capture: true },
)

const contentStyle = computed(() => ({
  width: props.matchAnchorWidth ? 'var(--reka-popover-trigger-width)' : undefined,
  minWidth: props.minWidth !== undefined ? `${props.minWidth}px` : undefined,
}))
</script>

<template>
  <PopoverRoot :open="open" @update:open="onOpenChange">
    <PopoverPortal>
      <!-- as-child so our element's `role` wins over Reka's default
           dialog role (Slot lets the child's props win); Reka still
           merges its id, data-state, tabindex and the CSS variables. -->
      <PopoverContent
        as-child
        :reference="reference"
        :side="side"
        :align="align"
        :side-offset="offset"
        :collision-padding="8"
        @pointer-down-outside="onPointerDownOutside"
        @open-auto-focus="onOpenAutoFocus"
        @close-auto-focus="onCloseAutoFocus"
      >
        <div
          :role="role"
          :aria-label="ariaLabel"
          class="popover-inner z-overlay outline-none"
          :class="[popoverClass, noTransition && 'popover-inner--static']"
          :style="contentStyle"
        >
          <slot />
        </div>
      </PopoverContent>
    </PopoverPortal>
  </PopoverRoot>
</template>

<style>
/* Global rather than scoped: the content is portalled out of the
   component's scoped-style context.

   Motion runs from Reka's `data-state` and grows out of the corner
   nearest the anchor (`--reka-popover-content-transform-origin`).
   Enter is graceful, exit is snappy, the usual rhythm for transient
   surfaces. Shadow and a hint of backdrop blur are the surface's own;
   the consumer's popover-class owns background and border. */

.popover-inner {
  transform-origin: var(--reka-popover-content-transform-origin);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  box-shadow:
    0 16px 48px -8px rgba(0, 0, 0, 0.18),
    0 6px 16px -4px rgba(0, 0, 0, 0.10),
    0 2px 4px -1px rgba(0, 0, 0, 0.06);
}

.popover-inner[data-state='open'] {
  animation: popover-in 180ms cubic-bezier(0.32, 0.72, 0, 1);
}

.popover-inner[data-state='closed'] {
  animation: popover-out 120ms cubic-bezier(0.4, 0, 1, 1);
}

.popover-inner--static[data-state] {
  animation: none;
}

@keyframes popover-in {
  from { opacity: 0; transform: translateY(-6px) scale(0.97); }
  to { opacity: 1; transform: none; }
}

@keyframes popover-out {
  from { opacity: 1; transform: none; }
  to { opacity: 0; transform: translateY(-6px) scale(0.97); }
}

@media (prefers-reduced-motion: reduce) {
  .popover-inner[data-state] {
    animation-duration: 1ms;
  }
}
</style>
