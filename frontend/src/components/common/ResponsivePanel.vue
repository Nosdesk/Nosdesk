<script setup lang="ts">
/**
 * Viewport-aware secondary surface. Below the `md` breakpoint
 * the panel renders as a bottom sheet (slides up over the doc,
 * drag-down to dismiss, backdrop tap dismisses, body scroll
 * locked). At `md` and above it renders as the previous side
 * panel layout — inline next to its sibling so the document
 * area shrinks and the panel sits flush.
 *
 * Drag math, breakpoint reactivity, and dismiss-on-cross are
 * delegated to `useResponsiveSheet` so this component stays
 * focused on the panel chrome (header, close button, backdrop,
 * the side-panel layout). The same composable backs
 * `<ResponsiveMenu>` for popover-vs-bottom-sheet menus.
 */
import { computed, toRef } from 'vue'
import Icon from './Icon.vue'
import { useScrollLock } from '@/composables/useScrollLock'
import { useResponsiveSheet } from '@/composables/useResponsiveSheet'

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

const { isMobile, dragOffset, isDragging, handleListeners } = useResponsiveSheet({
  open: toRef(props, 'open'),
  onDismiss: () => emit('close'),
})

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
        class="fixed inset-0 z-backdrop bg-black/40"
        aria-hidden="true"
        @click="emit('close')"
      />
    </Transition>
    <Transition name="sheet">
      <div v-if="open && isMobile" class="fixed inset-x-0 bottom-0 z-overlay">
        <!-- Inner panel carries the drag offset; the Vue transition slides the
             wrapper on enter/leave. Two elements so the transforms compose. -->
        <div
          class="flex h-[75vh] flex-col rounded-t-xl border-t border-default bg-surface shadow-2xl"
          :class="{ 'sheet-panel-settle': !isDragging }"
          :style="{ transform: `translateY(${dragOffset}px)` }"
          role="dialog"
          :aria-label="ariaLabel"
        >
        <!-- Drag handle. Big touch target, but visually a small
             pill so the chrome stays restrained. -->
        <div
          class="flex flex-shrink-0 cursor-grab items-center justify-center pt-2 pb-1 select-none active:cursor-grabbing"
          v-on="handleListeners"
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
        <!-- Bottom padding clears the iPhone home indicator (dead zone). -->
        <div class="flex flex-1 flex-col overflow-y-auto pb-[calc(0.5rem+env(safe-area-inset-bottom))]">
          <slot />
        </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style>
/* Sheet present/dismiss slide, shared by <ResponsiveMenu> and <ResponsivePanel>.
   Global (not scoped) because the sheet teleports out of scoped-style context.
   Present decelerates into place on the iOS present curve over a longer beat so
   it reads as rising, not popping; dismiss is a touch quicker. The slide lives on
   the WRAPPER; the drag offset lives on the inner panel (.sheet-panel-settle), so
   the two transforms never fight. */
.sheet-enter-active {
  transition: transform 360ms cubic-bezier(0.32, 0.72, 0, 1);
}
.sheet-leave-active {
  transition: transform 280ms cubic-bezier(0.32, 0.72, 0, 1);
}
.sheet-enter-from,
.sheet-leave-to {
  transform: translateY(100%);
}

/* Drag spring-back on release (no dismiss). Same curve as the present, a hair
   shorter. Absent mid-drag so the panel tracks the finger 1:1. */
.sheet-panel-settle {
  transition: transform 320ms cubic-bezier(0.32, 0.72, 0, 1);
}

.sheet-backdrop-enter-active {
  transition: opacity 360ms ease-out;
}
.sheet-backdrop-leave-active {
  transition: opacity 280ms ease-in;
}
.sheet-backdrop-enter-from,
.sheet-backdrop-leave-to {
  opacity: 0;
}
</style>
