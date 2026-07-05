<script setup lang="ts">
/**
 * Generic floating-element primitive. Pairs `usePopover` for
 * positioning with the standard interactive shell every popover
 * needs: teleport to body, click-outside dismiss, escape
 * dismiss, focus on mount, scroll/resize behaviour.
 *
 * Architecture — wrapper / inner separation:
 *
 *   <wrapper>           <- positioned by usePopover (left/top)
 *     <inner>           <- transitions (transform, opacity, shadow)
 *       <slot />
 *     </inner>
 *   </wrapper>
 *
 * The wrapper owns `position: fixed; left: <x>px; top: <y>px;`
 * and never animates. The inner element owns the transforms /
 * opacity / shadow / backdrop blur and never moves layout. This
 * is the pattern floating-ui's docs recommend specifically to
 * avoid transform conflicts between positioning and animation
 * (https://floating-ui.com/docs/vue) — and as a side-effect it
 * gives us a stable reference frame for `transform-origin` so
 * the scale/translate animation appears to grow out of the
 * trigger corner cleanly.
 *
 * Mount sequence — measured-then-animated:
 *
 *   1. props.open flips true
 *   2. `rendered.value = true` mounts the wrapper + inner in
 *      their initial (invisible) state
 *   3. `nextTick()` — DOM updated, refs populated
 *   4. `update()` measures + positions the wrapper
 *   5. Two rAFs — let the browser commit the initial state so
 *      the transition isn't elided as a "no animation needed"
 *   6. `visible.value = true` flips a class on the inner;
 *      CSS transition runs to the final state
 *
 * The two-rAF defer is the cross-browser standard for "I just
 * inserted an element, now I want a CSS transition to play."
 * Without it Chromium and Safari sometimes batch the two state
 * changes and skip the animation entirely.
 *
 * Two scroll strategies, picked per use case:
 *   - `'reposition'` — track the anchor across page scroll
 *     (good for trigger-anchored dropdowns where the trigger
 *     stays in the layout)
 *   - `'close'` — dismiss instead of repositioning (good for
 *     click-anchored menus where the anchor is a stale point)
 */
import { computed, nextTick, onScopeDispose, ref, shallowRef, watch } from 'vue'
import {
  usePopover,
  type PopoverAnchor,
  type PopoverPlacement,
} from '@/composables/usePopover'
import { useEventListener } from '@/composables/useEventListener'
import { registerPopover, unregisterPopover, isInNestedPopover } from '@/composables/usePopoverStack'

interface Props {
  /** Open state. Parent owns it; we render conditionally on
   * truthy. Watching it triggers the measure-then-animate
   * sequence. */
  open: boolean
  /** What we anchor against. Reactively re-read on each update. */
  anchor: PopoverAnchor
  /** Preferred placement; flips vertically if it doesn't fit. */
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

const { popoverRef, x, y, width, update, placement } = usePopover({
  anchor: () => props.anchor,
  placement: props.placement,
  matchAnchorWidth: props.matchAnchorWidth,
  minWidth: props.minWidth,
  offset: props.offset,
})

// shallowRef — these point at DOM elements, not reactive data.
// Wrapping them in a deep ref would have Vue walk the element
// tree on every update for no benefit.
const innerEl = shallowRef<HTMLDivElement | null>(null)

// Two-stage open state. `rendered` controls v-if (DOM presence);
// `visible` controls the CSS class that triggers the transition.
// Splitting them lets us mount, measure, paint, then animate —
// the canonical "transition an inserted element" pattern.
const rendered = ref(false)
const visible = ref(false)
const previouslyFocused = shallowRef<HTMLElement | null>(null)
const isOpen = computed(() => props.open)

// Map placement to transform-origin. The fade-scale-translate
// origin should be the corner nearest the anchor so the popover
// appears to emerge from the trigger rather than always from
// the top-left.
const transformOrigin = computed<string>(() => {
  const p = placement.value
  if (p.startsWith('top')) {
    if (p.endsWith('end')) return 'bottom right'
    if (p.endsWith('start')) return 'bottom left'
    return 'bottom center'
  }
  if (p.startsWith('bottom')) {
    if (p.endsWith('end')) return 'top right'
    if (p.endsWith('start')) return 'top left'
    return 'top center'
  }
  return 'top left'
})

// ---------------------------------------------------------------
// Dismiss handlers — gated by `isOpen` via useEventListener so
// listeners only exist while the popover is open.
// ---------------------------------------------------------------
function onMousedownOutside(event: MouseEvent): void {
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
  // Nested popovers: anything opened after us (deeper in the stack)
  // is logically our child even though it teleports to body, so a
  // click inside it must not dismiss us. A DatePicker's calendar
  // opened from inside this popover is the canonical case.
  if (isInNestedPopover(popoverRef.value, target)) return
  emit('close')
}

function registerOpen(): void {
  if (popoverRef.value) registerPopover(popoverRef.value)
}

function unregisterOpen(): void {
  if (popoverRef.value) unregisterPopover(popoverRef.value)
}

onScopeDispose(unregisterOpen)

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== 'Escape') return
  // Capture phase + stopPropagation so a popover nested inside a
  // modal (which also listens for Escape on document) closes first
  // without dismissing the modal underneath.
  event.preventDefault()
  event.stopPropagation()
  emit('close')
}

function onScroll(): void {
  if (props.reactToScroll === 'close') emit('close')
  else update()
}

function onResize(): void {
  // Resize is always disruptive enough to warrant a reposition
  // even for click-anchored popovers; the click point is still
  // valid (it was a viewport coordinate), the popover dims may
  // have changed.
  update()
}

// Retarget while open: a consumer that swaps its anchor (a hover
// card moving between gantt bars, say) passes a new anchor object;
// reposition against it without a close/reopen cycle.
watch(
  () => props.anchor,
  () => {
    if (props.open && rendered.value) update()
  },
)

useEventListener(document, 'mousedown', onMousedownOutside, { when: isOpen })
useEventListener(document, 'keydown', onKeydown, { when: isOpen, capture: true })
useEventListener(window, 'scroll', onScroll, { when: isOpen, capture: true })
useEventListener(window, 'resize', onResize, { when: isOpen })

// ---------------------------------------------------------------
// Open / close lifecycle. Watcher kicks off the measure-then-
// animate sequence on open and the leave transition on close.
// ---------------------------------------------------------------
let leaveTimer: ReturnType<typeof setTimeout> | null = null

const LEAVE_MS = 180  // matches longest leave transition + safety

watch(
  () => props.open,
  async (open) => {
    if (open) {
      if (leaveTimer) {
        clearTimeout(leaveTimer)
        leaveTimer = null
      }
      previouslyFocused.value = (document.activeElement as HTMLElement) ?? null
      rendered.value = true
      visible.value = false
      // Wait for the wrapper + inner to mount.
      await nextTick()
      // Position the wrapper now, while the inner is still in
      // its enter-from state. By the time visible flips true the
      // wrapper is already in the right place — the transition
      // only animates the inner's transform / opacity / shadow.
      update()
      // Now that popoverRef is mounted, join the open stack so any
      // popover we open in turn nests beneath us.
      registerOpen()
      if (props.noTransition) {
        visible.value = true
        if (props.autoFocus) popoverRef.value?.focus()
        return
      }
      // Two requestAnimationFrame ticks: the canonical pattern
      // for "I just inserted an element, force the browser to
      // commit the initial state, then change to the target so
      // a CSS transition runs." A single rAF batches both state
      // changes in some engines and the transition is skipped.
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          visible.value = true
          if (props.autoFocus) popoverRef.value?.focus()
        })
      })
    } else {
      // Leave the stack at once on close, before the leave
      // transition plays, so we're not treated as open during it.
      unregisterOpen()
      // Don't unmount immediately — let the leave transition
      // play first, then drop the v-if.
      visible.value = false
      if (props.autoFocus) {
        previouslyFocused.value?.focus?.()
        previouslyFocused.value = null
      }
      if (leaveTimer) clearTimeout(leaveTimer)
      leaveTimer = setTimeout(() => {
        rendered.value = false
        leaveTimer = null
      }, props.noTransition ? 0 : LEAVE_MS)
    }
  },
  { immediate: true },
)
</script>

<template>
  <Teleport to="body">
    <!-- Wrapper: positioned by usePopover. Stays static during
         transition so the inner element has a stable reference
         frame for its scale/translate. usePopover measures
         against this element via popoverRef. -->
    <div
      v-if="rendered"
      ref="popoverRef"
      class="popover-wrapper fixed z-overlay outline-none"
      :role="role"
      :aria-label="ariaLabel"
      :tabindex="-1"
      :style="{
        left: `${x}px`,
        top: `${y}px`,
        width: width !== null ? `${width}px` : undefined,
      }"
    >
      <!-- Inner: animated. Owns the transform / opacity / shadow
           transitions. Class flip on `visible` triggers the
           CSS transition from initial to settled state. -->
      <div
        ref="innerEl"
        class="popover-inner"
        :class="[popoverClass, visible && 'popover-inner--visible']"
        :style="{ transformOrigin }"
      >
        <slot />
      </div>
    </div>
  </Teleport>
</template>

<style>
/* Global rather than scoped: these classes apply to the popover
   root which is teleported out of the component's scoped-style
   context.

   Why two transitions per property pair:
   - opacity moves on a faster, simpler curve (120/100ms ease)
     so the popover registers as "appearing" quickly
   - transform moves on a slower, more shaped curve (220ms
     cubic-bezier(0.32, 0.72, 0, 1) — the same family Apple's
     SwiftUI uses for sheet presentations) so the motion settles
     gracefully without overshooting
   - box-shadow grows in lockstep with the transform so the
     popover gains depth as it moves into place

   The leave transition is shorter and uses an ease-in curve
   ("appear graceful, dismiss snappy") — standard rhythm for
   transient surfaces. */

.popover-inner {
  /* Settled state. The class flip animates from --enter-from
     values (set in popover-inner:not(.popover-inner--visible))
     to these. */
  opacity: 0;
  transform: translateY(-6px) scale(0.97);
  /* Subtle frosted-glass effect over scrolling content. The
     consumer's popover-class still owns background colour
     (usually bg-surface) — backdrop-filter just adds a hint of
     blur for content behind a partially transparent surface. */
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  box-shadow:
    0 4px 12px -2px rgba(0, 0, 0, 0.08),
    0 2px 4px -1px rgba(0, 0, 0, 0.04);
  transition:
    opacity 100ms cubic-bezier(0.4, 0, 1, 1),
    transform 140ms cubic-bezier(0.4, 0, 1, 1),
    box-shadow 140ms cubic-bezier(0.4, 0, 1, 1);
}

.popover-inner--visible {
  opacity: 1;
  transform: translateY(0) scale(1);
  box-shadow:
    0 16px 48px -8px rgba(0, 0, 0, 0.18),
    0 6px 16px -4px rgba(0, 0, 0, 0.10),
    0 2px 4px -1px rgba(0, 0, 0, 0.06);
  transition:
    opacity 120ms ease-out,
    transform 220ms cubic-bezier(0.32, 0.72, 0, 1),
    box-shadow 220ms cubic-bezier(0.32, 0.72, 0, 1);
}

@media (prefers-reduced-motion: reduce) {
  .popover-inner {
    transition: opacity 100ms linear !important;
    transform: none !important;
  }
  .popover-inner--visible {
    transition: opacity 100ms linear !important;
    transform: none !important;
  }
}
</style>
