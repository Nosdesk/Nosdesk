<script setup lang="ts">
/**
 * Viewport-aware floating surface. Below `md` it renders as a bottom
 * sheet (`BottomSheet`, a modal Reka Dialog with drag-to-dismiss). At
 * `md` and above it renders anchored: a Reka DropdownMenu when `role` is
 * `menu` (roving focus, typeahead, Enter/Space select on `MenuItem` /
 * `MenuList` rows), otherwise a `Popover` carrying the given role
 * (`listbox` for dropdown option lists, `dialog` for panels with
 * controls or inputs). All three share the same slot, so consumers write
 * the content once.
 *
 * `role` is therefore load-bearing: a surface that contains text inputs
 * or arbitrary controls must not claim `menu`, or the menu keyboard model
 * (typeahead on printable keys, arrow roving) fights the controls.
 */
import { computed, toRef } from 'vue'
import Popover from './Popover.vue'
import DropdownSurface from './DropdownSurface.vue'
import BottomSheet from './BottomSheet.vue'
import { useResponsiveSheet } from '@/composables/useResponsiveSheet'
import type { PopoverAnchor, PopoverPlacement } from '@/composables/popoverAnchor'

interface Props {
  open: boolean
  /** Anchor for the desktop layout. Ignored on mobile (sheets are
   * screen-anchored, not element-anchored). */
  anchor: PopoverAnchor
  /** Optional sheet header label. Bottom sheets read better with a
   * title; popovers usually don't need one (the trigger provides the
   * context). */
  title?: string
  // ---- Pass-through to the desktop surface --------------------
  placement?: PopoverPlacement
  reactToScroll?: 'close' | 'reposition'
  matchAnchorWidth?: boolean
  minWidth?: number
  offset?: number
  /** Move keyboard focus into the surface on open. Right default for
   * action menus; dropdowns that keep focus on the trigger pass false. */
  autoFocus?: boolean
  role?: 'menu' | 'listbox' | 'dialog'
  ariaLabel?: string
  /** Tailwind chrome for the desktop surface. The bottom sheet has its
   * own chrome (rounded top, shadow, drag handle). */
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

// Breakpoint reactivity and the close-on-breakpoint-cross policy; the
// drag state this also returns is used by BottomSheet's own instance.
const { isMobile } = useResponsiveSheet({
  open: toRef(props, 'open'),
  onDismiss: () => emit('close'),
})
const ariaLabel = computed(() => props.ariaLabel ?? props.title)
</script>

<template>
  <BottomSheet
    v-if="isMobile"
    :open="open"
    :title="title"
    :aria-label="ariaLabel"
    :body-role="role === 'menu' ? 'menu' : undefined"
    @close="emit('close')"
  >
    <slot />
  </BottomSheet>

  <DropdownSurface
    v-else-if="role === 'menu'"
    :open="open"
    :anchor="anchor"
    :placement="placement"
    :react-to-scroll="reactToScroll"
    :match-anchor-width="matchAnchorWidth"
    :min-width="minWidth"
    :offset="offset"
    :auto-focus="autoFocus"
    :aria-label="ariaLabel"
    :popover-class="popoverClass"
    @close="emit('close')"
  >
    <slot />
  </DropdownSurface>

  <Popover
    v-else
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
</template>
