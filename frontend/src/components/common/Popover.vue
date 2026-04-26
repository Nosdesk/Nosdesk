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
  /** Move keyboard focus to the popover root on open. Right
   * default for context menus and dialogs (focus trap-style:
   * arrows/escape are handled inside the popover). Wrong for
   * dropdowns where focus should stay on the trigger so the
   * trigger's keydown handler keeps owning arrow navigation —
   * pass `false` in that case. */
  autoFocus?: boolean
  /** Disable the default fade-scale enter/leave transition. The
   * primitive includes a tasteful default so consumers don't
   * each reinvent one; this escape hatch is for cases where the
   * caller wants no animation (tests, motion-reduced overrides). */
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
  const target = event.target as Node
  if (popoverRef.value.contains(target)) return
  // Don't dismiss when the click landed on the anchor element
  // itself. Without this, a trigger button would close-then-
  // reopen on every click (mousedown closes the popover, then
  // the click event re-fires the trigger's toggle handler), and
  // the toggle would never feel like a toggle.
  const a = props.anchor
  if (a.type === 'element') {
    const el = a.element()
    if (el && el.contains(target)) return
  }
  emit('close')
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
      if (props.autoFocus) {
        popoverRef.value?.focus()
      }
    } else {
      detachListeners()
      if (props.autoFocus) {
        // Only return focus if we took it. Dropdowns leave focus
        // on the trigger throughout, so the explicit return would
        // be a no-op (or worse, blur-then-refocus a control the
        // user is actively typing into).
        previouslyFocused.value?.focus?.()
      }
      previouslyFocused.value = null
    }
  },
  { immediate: true },
)

onBeforeUnmount(detachListeners)
</script>

<template>
  <Teleport to="body">
    <Transition
      :name="noTransition ? '' : 'popover-fade-scale'"
      :duration="noTransition ? 0 : undefined"
    >
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
    </Transition>
  </Teleport>
</template>

<style>
/* Global rather than scoped: the Transition class names need to
   apply to the popover root which is teleported out of the
   component's scoped-style context. The transform-origin uses
   the top-left corner so the popover appears to grow out of its
   anchor point — feels right for both dropdowns (anchored to
   the trigger's top-left) and click-anchored menus (anchored
   to the click point). */
.popover-fade-scale-enter-active {
  transition:
    opacity 100ms ease-out,
    transform 100ms ease-out;
  transform-origin: top left;
}
.popover-fade-scale-leave-active {
  transition:
    opacity 75ms ease-in,
    transform 75ms ease-in;
  transform-origin: top left;
}
.popover-fade-scale-enter-from,
.popover-fade-scale-leave-to {
  opacity: 0;
  transform: scale(0.95);
}
</style>
