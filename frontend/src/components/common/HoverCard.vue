<script setup lang="ts">
/**
 * Floating hover-preview surface: a Popover configured for
 * tooltip-like use (no focus steal, top placement, one shared
 * elevation), driven by a `useHoverCard` controller. Content is
 * supplementary detail; the target keeps its own aria-label with
 * the essentials, so the card can stay pointer-first.
 *
 * Pointer entering the card keeps it open (the controller's grace
 * window), and Escape or scroll-away dismisses; both satisfy WCAG
 * 1.4.13 hoverable/dismissable.
 */
import { computed } from 'vue'
import Popover from '@/components/common/Popover.vue'
import type { PopoverPlacement } from '@/composables/usePopover'

const props = withDefaults(
  defineProps<{
    open: boolean
    anchor: HTMLElement | null
    placement?: PopoverPlacement
    ariaLabel?: string
  }>(),
  { placement: 'top-start' },
)

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'card-enter'): void
  (e: 'card-leave'): void
}>()

// New object per anchor element, so Popover's anchor watch
// repositions on retarget without a close/reopen cycle.
const anchorRef = computed(() => ({
  type: 'element' as const,
  element: () => props.anchor,
}))
</script>

<template>
  <Popover
    :open="open && !!anchor"
    :anchor="anchorRef"
    :placement="placement"
    role="tooltip"
    :aria-label="ariaLabel"
    :auto-focus="false"
    react-to-scroll="reposition"
    popover-class="bg-surface border border-default rounded-lg"
    @close="emit('close')"
  >
    <div @pointerenter="emit('card-enter')" @pointerleave="emit('card-leave')">
      <slot />
    </div>
  </Popover>
</template>
