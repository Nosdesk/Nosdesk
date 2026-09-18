<!--
Mobile bottom sheet: a modal Reka Dialog styled as a sheet. Reka owns the
focus trap, the page hiding, body scroll lock, Escape and backdrop
dismiss and focus restore; `useResponsiveSheet` keeps the drag-to-dismiss
handle. The slot content is plain (buttons on the menu recipe), which is
the interim design while Reka's Drawer is alpha; swipe momentum and snap
points come back with it.

`title` is the accessible name and the visible header; when a surface has
no visible title it still names the sheet through `ariaLabel`.
-->
<script setup lang="ts">
import { computed, toRef } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  DialogContent,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
  VisuallyHidden,
} from 'reka-ui'
import { useResponsiveSheet } from '@/composables/useResponsiveSheet'

interface Props {
  open: boolean
  title?: string
  ariaLabel?: string
  /** Role for the scrolling body, e.g. `menu` around menu rows. */
  bodyRole?: string
}

const props = defineProps<Props>()
const emit = defineEmits<{ (e: 'close'): void }>()

const { dragOffset, isDragging, handleListeners } = useResponsiveSheet({
  open: toRef(props, 'open'),
  onDismiss: () => emit('close'),
})

function onOpenChange(open: boolean) {
  if (!open) emit('close')
}

// A dialog needs a name even when the surface has no visible title.
const fluent = useFluent()
const hiddenTitle = computed(() => props.ariaLabel ?? fluent.$t('common-sheet-aria'))
</script>

<template>
  <DialogRoot :open="open" @update:open="onOpenChange">
    <DialogPortal>
      <DialogOverlay class="bottom-sheet__backdrop fixed inset-0 z-backdrop bg-black/40" />
      <!-- as-child: the fixed wrapper is ours so the slide keyframe and
           the drag transform live on separate elements. Reka names it
           (aria-labelledby the title) and traps focus inside. -->
      <DialogContent as-child :aria-describedby="undefined">
        <div class="bottom-sheet fixed inset-x-0 bottom-0 z-overlay outline-none" aria-modal="true">
          <div
            class="flex max-h-[80vh] flex-col rounded-t-xl border-t border-default bg-surface shadow-2xl"
            :class="{ 'sheet-panel-settle': !isDragging }"
            :style="{ transform: `translateY(${dragOffset}px)` }"
          >
            <!-- Drag handle pill. The tappable area extends beyond the
                 pill so the user has more thumb room. -->
            <div
              class="flex flex-shrink-0 cursor-grab items-center justify-center pt-2 pb-1 select-none active:cursor-grabbing"
              v-on="handleListeners"
            >
              <div class="h-1 w-10 rounded-full bg-border"></div>
            </div>
            <DialogTitle
              v-if="title"
              as="h2"
              class="flex-shrink-0 px-4 pt-1 pb-3 text-sm font-semibold text-primary"
            >
              {{ title }}
            </DialogTitle>
            <VisuallyHidden v-else as-child>
              <DialogTitle>{{ hiddenTitle }}</DialogTitle>
            </VisuallyHidden>
            <!-- Bottom padding clears the iPhone home indicator so the last
                 row is not in the dead zone. -->
            <div
              class="flex flex-1 flex-col overflow-y-auto pb-[calc(0.5rem+env(safe-area-inset-bottom))]"
              :role="bodyRole"
              :aria-label="bodyRole ? (title ?? hiddenTitle) : undefined"
            >
              <slot />
            </div>
          </div>
        </div>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

<style>
/* Global: portalled out of scoped-style context. The slide lives on the
   wrapper (data-state keyframes), the drag offset on the inner panel, so
   the two transforms never fight. Present rises on the iOS curve over a
   longer beat; dismiss is a touch quicker. */
.bottom-sheet[data-state='open'] {
  animation: bottom-sheet-in 360ms cubic-bezier(0.32, 0.72, 0, 1);
}
.bottom-sheet[data-state='closed'] {
  animation: bottom-sheet-out 280ms cubic-bezier(0.32, 0.72, 0, 1);
}
.bottom-sheet__backdrop[data-state='open'] {
  animation: bottom-sheet-fade-in 360ms ease-out;
}
.bottom-sheet__backdrop[data-state='closed'] {
  animation: bottom-sheet-fade-out 280ms ease-in;
}

@keyframes bottom-sheet-in { from { transform: translateY(100%); } to { transform: none; } }
@keyframes bottom-sheet-out { from { transform: none; } to { transform: translateY(100%); } }
@keyframes bottom-sheet-fade-in { from { opacity: 0; } to { opacity: 1; } }
@keyframes bottom-sheet-fade-out { from { opacity: 1; } to { opacity: 0; } }

/* Drag spring-back on release (no dismiss). Absent mid-drag so the panel
   tracks the finger 1:1. */
.sheet-panel-settle {
  transition: transform 320ms cubic-bezier(0.32, 0.72, 0, 1);
}

@media (prefers-reduced-motion: reduce) {
  .bottom-sheet[data-state],
  .bottom-sheet__backdrop[data-state] {
    animation-duration: 1ms;
  }
}
</style>
