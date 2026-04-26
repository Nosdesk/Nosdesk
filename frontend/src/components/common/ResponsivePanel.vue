<script setup lang="ts">
/**
 * Viewport-aware secondary surface. Below the `md` breakpoint
 * the panel renders as a bottom sheet (slides up over the doc,
 * drag-down to dismiss, backdrop tap dismisses, body scroll
 * locked). At `md` and above it renders as the previous side
 * panel layout — inline next to its sibling so the document
 * area shrinks and the panel sits flush.
 *
 * The two layouts are intentionally NOT animated as one
 * morphing thing across the breakpoint: cross-breakpoint
 * resizes are rare (orientation change, window resize), and
 * morphing a 320px right-side panel into a 75vh bottom sheet
 * always looks worse than just dismissing and reopening. So we
 * just close on resize-across-breakpoint.
 *
 * Slot contract:
 *   - default slot: panel body
 *   - The component owns the chrome (header bar, drag handle,
 *     close button, backdrop) so consumers don't reinvent them.
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import Icon from './Icon.vue'
import { useScrollLock } from '@/composables/useScrollLock'

interface Props {
  open: boolean
  /** Header label. Required — anchors the panel for the user
   * regardless of which layout fires. */
  title: string
  /** Tailwind sizing applied in the side-panel layout only. The
   * bottom sheet always sizes itself (75vh by default; drag to
   * dismiss). */
  sidePanelClass?: string
  /** Accessible label for the panel itself. Defaults to `title`. */
  ariaLabel?: string
}

const props = withDefaults(defineProps<Props>(), {
  sidePanelClass: 'w-80',
})

const emit = defineEmits<{ (e: 'close'): void }>()

// -----------------------------------------------------------------
// Layout selector. matchMedia drives a reactive boolean so the
// template renders the right shape even if the viewport changes
// while the panel is open.
// -----------------------------------------------------------------
const isMobile = ref(false)
let mql: MediaQueryList | null = null

function syncBreakpoint(e: MediaQueryListEvent | MediaQueryList) {
  // `min-width: 768px` (Tailwind's `md`). At-or-above is desktop;
  // below is bottom-sheet territory.
  isMobile.value = !e.matches
}

onMounted(() => {
  if (typeof window === 'undefined') return
  mql = window.matchMedia('(min-width: 768px)')
  syncBreakpoint(mql)
  mql.addEventListener('change', syncBreakpoint)
})

onUnmounted(() => {
  mql?.removeEventListener('change', syncBreakpoint)
  mql = null
})

// If the breakpoint flips while the panel is open, dismiss
// rather than morph. Cleaner UX than a 320px → 75vh slide.
watch(isMobile, (_now, prev) => {
  if (props.open && prev !== undefined) emit('close')
})

// -----------------------------------------------------------------
// Bottom-sheet drag math. Drag the handle (NOT the content) to
// avoid hijacking scroll. Sheet only translates downward — the
// `Math.max(0, …)` clamp prevents up-drag, which would feel
// like a half-baked expand gesture. Past `DISMISS_THRESHOLD_PX`
// of travel on release we close; otherwise spring back.
// -----------------------------------------------------------------
const DISMISS_THRESHOLD_PX = 100
const dragOffset = ref(0)
const isDragging = ref(false)
let dragStartY = 0

function onHandlePointerdown(event: PointerEvent) {
  if (event.button !== 0) return
  isDragging.value = true
  dragStartY = event.clientY
  ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
}

function onHandlePointermove(event: PointerEvent) {
  if (!isDragging.value) return
  dragOffset.value = Math.max(0, event.clientY - dragStartY)
}

function onHandlePointerup() {
  if (!isDragging.value) return
  isDragging.value = false
  if (dragOffset.value > DISMISS_THRESHOLD_PX) {
    emit('close')
  }
  // Whether dismissed or not, reset the drag state. On dismiss
  // the panel unmounts via the parent's `open` flip, so the
  // visible "snap back" only happens on cancelled drags.
  dragOffset.value = 0
}

// Reset offset whenever the panel opens, so a previous drag
// never leaks into a fresh open.
watch(
  () => props.open,
  (open) => {
    if (open) dragOffset.value = 0
  },
)

// Lock body scroll while the bottom sheet is open. Prevents the
// underlying document from scrolling under the user's finger
// when they drag the sheet (which would be a surprising
// gesture-conflict). On desktop the side-panel layout doesn't
// need this — the document area shrinks but stays scrollable
// independently.
const shouldLockBodyScroll = computed(() => props.open && isMobile.value)
useScrollLock(shouldLockBodyScroll)

const ariaLabel = computed(() => props.ariaLabel ?? props.title)
</script>

<template>
  <!-- Side panel: inline at md+; sits flush next to its sibling
       so the document area shrinks and the panel docks against
       the right edge. No teleport — flex layout above us
       arranges the columns. -->
  <aside
    v-if="open && !isMobile"
    class="flex h-full flex-shrink-0 flex-col border-l border-default bg-surface"
    :class="sidePanelClass"
    role="complementary"
    :aria-label="ariaLabel"
  >
    <header class="flex items-center justify-between border-b border-default px-4 py-3">
      <h2 class="text-sm font-semibold text-primary">{{ title }}</h2>
      <button
        type="button"
        @click="emit('close')"
        :aria-label="`Close ${title.toLowerCase()}`"
        class="rounded p-1 text-tertiary transition-colors hover:bg-surface-hover hover:text-primary"
      >
        <Icon name="close" size="sm" />
      </button>
    </header>
    <div class="flex flex-1 flex-col overflow-y-auto">
      <slot />
    </div>
  </aside>

  <!-- Bottom sheet: teleport to body so it overlays the doc
       regardless of the local layout. Backdrop fades in; sheet
       slides up. Drag the handle (not the content) to dismiss. -->
  <Teleport to="body">
    <Transition name="sheet-backdrop">
      <div
        v-if="open && isMobile"
        class="fixed inset-0 z-[90] bg-black/40"
        aria-hidden="true"
        @click="emit('close')"
      />
    </Transition>
    <Transition name="sheet">
      <div
        v-if="open && isMobile"
        class="fixed inset-x-0 bottom-0 z-[91] flex h-[75vh] flex-col rounded-t-xl border-t border-default bg-surface shadow-2xl"
        :class="{ 'transition-transform': !isDragging }"
        :style="{ transform: `translateY(${dragOffset}px)` }"
        role="dialog"
        :aria-label="ariaLabel"
      >
        <!-- Drag handle. Big touch target, but visually a small
             pill so the chrome stays restrained. The full header
             area is the drag target so the user has more thumb
             room than the pill itself. -->
        <div
          class="flex flex-shrink-0 cursor-grab items-center justify-center pt-2 pb-1 select-none active:cursor-grabbing"
          @pointerdown="onHandlePointerdown"
          @pointermove="onHandlePointermove"
          @pointerup="onHandlePointerup"
          @pointercancel="onHandlePointerup"
        >
          <div class="h-1 w-10 rounded-full bg-border"></div>
        </div>
        <header class="flex flex-shrink-0 items-center justify-between px-4 pb-3">
          <h2 class="text-sm font-semibold text-primary">{{ title }}</h2>
          <button
            type="button"
            @click="emit('close')"
            :aria-label="`Close ${title.toLowerCase()}`"
            class="rounded p-1 text-tertiary transition-colors hover:bg-surface-hover hover:text-primary"
          >
            <Icon name="close" size="sm" />
          </button>
        </header>
        <div class="flex flex-1 flex-col overflow-y-auto">
          <slot />
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style>
/* Sheet enter/leave: 200ms upward slide for sheet, paired
   backdrop fade. Global rather than scoped because the sheet is
   teleported out of the component's scoped-style context. */
.sheet-enter-active,
.sheet-leave-active {
  transition: transform 200ms cubic-bezier(0.16, 1, 0.3, 1);
}
.sheet-enter-from,
.sheet-leave-to {
  transform: translateY(100%);
}

.sheet-backdrop-enter-active,
.sheet-backdrop-leave-active {
  transition: opacity 200ms ease-out;
}
.sheet-backdrop-enter-from,
.sheet-backdrop-leave-to {
  opacity: 0;
}
</style>
