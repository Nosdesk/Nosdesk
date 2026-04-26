<script setup lang="ts">
/**
 * Generic floating-element primitive. Pairs `usePopover` for
 * positioning with the standard interactive shell every popover
 * needs: teleport to body, click-outside dismiss, escape
 * dismiss, focus on mount, scroll/resize behaviour.
 *
 * Two scroll strategies, picked per use case:
 *   - `'reposition'` — track the anchor across page scroll
 *     (good for trigger-anchored dropdowns where the trigger
 *     stays in the layout)
 *   - `'close'` — dismiss instead of repositioning (good for
 *     click-anchored menus where the anchor is a stale point)
 *
 * The component owns the chrome — positioning, dismiss, focus,
 * ARIA. Content is the consumer's job, passed as default slot.
 */
import { nextTick, onBeforeUnmount, ref, watch } from 'vue'
import {
  usePopover,
  type PopoverAnchor,
  type PopoverPlacement,
} from '@/composables/usePopover'

interface Props {
  /** Open state. The parent owns it; we render conditionally on
   * truthy. Watching it lets us re-position on each open and
   * focus the popover for keyboard users. */
  open: boolean
  /** What we anchor against. Reactively re-read on each update. */
  anchor: PopoverAnchor
  /** Preferred placement; flips vertically if it doesn't fit. */
  placement?: PopoverPlacement
  /** What happens when the page scrolls. Click-anchored popovers
   * almost always want `'close'`; element-anchored dropdowns want
   * `'reposition'`. */
  reactToScroll?: 'close' | 'reposition'
  /** Match anchor's width — for select-style dropdowns. */
  matchAnchorWidth?: boolean
  /** Minimum width when `matchAnchorWidth` is on. */
  minWidth?: number
  /** Pixel gap between anchor and popover. */
  offset?: number
  /** ARIA role applied to the popover root. Pick one that
   * matches the content: `menu` for action lists, `listbox` for
   * select-style options, `dialog` for arbitrary panels. */
  role?: 'menu' | 'listbox' | 'dialog'
  /** Accessible name. Required for `dialog`; optional for menu /
   * listbox where the trigger usually labels them via
   * `aria-controls`. */
  ariaLabel?: string
  /** Tailwind / arbitrary class on the floating element. Lets the
   * consumer pick the surface chrome (border, shadow, padding)
   * without baking it into the primitive. */
  popoverClass?: string
}

const props = withDefaults(defineProps<Props>(), {
  placement: 'bottom-start',
  reactToScroll: 'reposition',
  matchAnchorWidth: false,
  offset: 4,
  role: 'menu',
})

const emit = defineEmits<{ (e: 'close'): void }>()

const { popoverRef, x, y, width, update } = usePopover({
  anchor: () => props.anchor,
  placement: props.placement,
  matchAnchorWidth: props.matchAnchorWidth,
  minWidth: props.minWidth,
  offset: props.offset,
})

// -----------------------------------------------------------------
// Dismiss handlers — wired up only while open so the listeners
// don't leak when the popover is closed.
// -----------------------------------------------------------------
function onMousedownOutside(event: MouseEvent) {
  if (!popoverRef.value) return
  if (!popoverRef.value.contains(event.target as Node)) {
    emit('close')
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') emit('close')
}

function onScroll() {
  if (props.reactToScroll === 'close') {
    emit('close')
  } else {
    update()
  }
}

function onResize() {
  // Resize is always disruptive enough to warrant a reposition
  // even for click-anchored popovers; the click point is still
  // valid (it was a viewport coordinate), the popover dims may
  // have changed.
  update()
}

function attachListeners() {
  document.addEventListener('mousedown', onMousedownOutside)
  document.addEventListener('keydown', onKeydown)
  // capture: true so we catch scrolls inside any ancestor
  // scroll container, not just the document.
  window.addEventListener('scroll', onScroll, true)
  window.addEventListener('resize', onResize)
}

function detachListeners() {
  document.removeEventListener('mousedown', onMousedownOutside)
  document.removeEventListener('keydown', onKeydown)
  window.removeEventListener('scroll', onScroll, true)
  window.removeEventListener('resize', onResize)
}

const previouslyFocused = ref<HTMLElement | null>(null)

watch(
  () => props.open,
  async (open) => {
    if (open) {
      // Save the current focus owner so we can return to it on
      // close — keyboard users land back where they started.
      previouslyFocused.value = (document.activeElement as HTMLElement) ?? null
      attachListeners()
      await nextTick()
      update()
      popoverRef.value?.focus()
    } else {
      detachListeners()
      previouslyFocused.value?.focus?.()
      previouslyFocused.value = null
    }
  },
  { immediate: true },
)

onBeforeUnmount(detachListeners)
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      ref="popoverRef"
      :role="role"
      :aria-label="ariaLabel"
      :tabindex="-1"
      class="fixed z-[100] outline-none"
      :class="popoverClass"
      :style="{
        left: `${x}px`,
        top: `${y}px`,
        width: width !== null ? `${width}px` : undefined,
      }"
    >
      <slot />
    </div>
  </Teleport>
</template>
