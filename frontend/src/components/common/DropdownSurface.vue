<!--
Desktop action-menu surface on Reka's DropdownMenu, driven by the same
open + anchor contract as `Popover` (no Reka trigger). Reka owns the menu
keyboard model: arrow keys move between rows with roving focus, Home/End,
typeahead on the row text, Enter/Space select, Escape closes, and the
dismiss layer stack. Rows are `MenuItem` (which detects this context and
becomes a `DropdownMenuItem`) or `MenuList`.

Non-modal: a click outside both closes the menu and lands, as the
hand-rolled menus did.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { DropdownMenuContent, DropdownMenuPortal, DropdownMenuRoot } from 'reka-ui'
import {
  anchorElementOf,
  floatingFrom,
  restoreFocusTo,
  type PopoverAnchor,
  type PopoverPlacement,
} from '@/composables/popoverAnchor'
import { useEventListener } from '@/composables/useEventListener'

interface Props {
  open: boolean
  anchor: PopoverAnchor
  placement?: PopoverPlacement
  reactToScroll?: 'close' | 'reposition'
  matchAnchorWidth?: boolean
  minWidth?: number
  offset?: number
  ariaLabel?: string
  popoverClass?: string
  /** Accepted for symmetry with Popover; focus always enters the menu
   *  (see onCloseAutoFocus). */
  autoFocus?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  placement: 'bottom-start',
  reactToScroll: 'reposition',
  matchAnchorWidth: false,
  offset: 4,
  autoFocus: true,
})

const emit = defineEmits<{ (e: 'close'): void }>()

const floating = computed(() => floatingFrom(props.anchor, props.placement))
const anchorElement = () => anchorElementOf(props.anchor)

function onOpenChange(open: boolean) {
  if (!open) emit('close')
}

// See Popover: the anchor is not a Reka trigger, so a pointerdown on it
// must not count as outside, or a toggle button would close-then-reopen.
let closedByOutsidePointer = false
function onPointerDownOutside(event: CustomEvent<{ originalEvent: PointerEvent }>) {
  const target = event.detail.originalEvent.target as Node | null
  const el = anchorElement()
  if (target && el?.contains(target)) event.preventDefault()
  else closedByOutsidePointer = true
}

function onOpenAutoFocus() {
  closedByOutsidePointer = false
}

// Focus always moves into the menu on open: the arrow keys, typeahead and
// Escape are handled on the content element, so a menu that left focus
// on its trigger would be a keyboard dead end (Radix behaves the same).
// Pointer users never see it: the container has no ring, and focus goes
// back to the anchor on close. `autoFocus=false` is therefore accepted
// for API symmetry with Popover but does not keep focus on the trigger.
function onCloseAutoFocus(event: Event) {
  event.preventDefault()
  if (!closedByOutsidePointer) restoreFocusTo(props.anchor)
  closedByOutsidePointer = false
}

useEventListener(
  window,
  'scroll',
  () => {
    if (props.reactToScroll === 'close') emit('close')
  },
  { when: computed(() => props.open), capture: true },
)

const contentStyle = computed(() => ({
  width: props.matchAnchorWidth ? 'var(--reka-dropdown-menu-trigger-width)' : undefined,
  minWidth: props.minWidth !== undefined ? `${props.minWidth}px` : undefined,
}))
</script>

<template>
  <DropdownMenuRoot :open="open" :modal="false" @update:open="onOpenChange">
    <DropdownMenuPortal>
      <DropdownMenuContent
        as-child
        loop
        :reference="floating.reference"
        :side="floating.side"
        :align="floating.align"
        :side-offset="offset"
        :collision-padding="8"
        @pointer-down-outside="onPointerDownOutside"
        @open-auto-focus="onOpenAutoFocus"
        @close-auto-focus="onCloseAutoFocus"
      >
        <div
          :aria-label="ariaLabel"
          class="popover-inner dropdown-surface z-overlay outline-none"
          :class="popoverClass"
          :style="contentStyle"
        >
          <slot />
        </div>
      </DropdownMenuContent>
    </DropdownMenuPortal>
  </DropdownMenuRoot>
</template>

<style>
/* Shares `.popover-inner`'s shadow, blur and data-state motion (global,
   in Popover.vue); the transform origin comes from the menu's own var. */
.dropdown-surface {
  transform-origin: var(--reka-dropdown-menu-content-transform-origin);
}
</style>
