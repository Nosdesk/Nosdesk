<script setup lang="ts">
/**
 * Viewport-aware secondary surface. Below the `md` breakpoint the
 * panel is `BottomSheet` (a modal Reka Dialog: focus trap, Escape,
 * page hidden from assistive tech, body scroll lock, drag-down to
 * dismiss). At `md` and above it renders as a side panel, inline next
 * to its sibling so the document area shrinks and the panel sits flush.
 *
 * Breakpoint reactivity and dismiss-on-cross come from
 * `useResponsiveSheet`, the same composable behind `<ResponsiveMenu>`.
 */
import { computed, toRef } from 'vue'
import Icon from './Icon.vue'
import BottomSheet from './BottomSheet.vue'
import { useResponsiveSheet } from '@/composables/useResponsiveSheet'

interface Props {
  open: boolean
  /** Header label. Required: it names the panel in both layouts. */
  title: string
  /** Tailwind sizing applied in the side-panel layout only. The
   * bottom sheet is a fixed 75vh so a list that filters as the user
   * types does not resize under the finger. */
  sidePanelClass?: string
  /** Accessible label for the panel itself. Defaults to `title`. */
  ariaLabel?: string
}

const props = withDefaults(defineProps<Props>(), {
  sidePanelClass: 'w-80',
})

const emit = defineEmits<{ (e: 'close'): void }>()

const { isMobile } = useResponsiveSheet({
  open: toRef(props, 'open'),
  onDismiss: () => emit('close'),
})

const ariaLabel = computed(() => props.ariaLabel ?? props.title)
</script>

<template>
  <!-- Side panel: inline at md+; sits flush next to its sibling so the
       document area shrinks and the panel docks against the right edge.
       No teleport; the flex layout above arranges the columns. -->
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
        :aria-label="$t('common-panel-close', { title })"
        class="rounded p-1 text-tertiary transition-colors hover:bg-surface-hover hover:text-primary"
      >
        <Icon name="close" size="sm" />
      </button>
    </header>
    <div class="flex flex-1 flex-col overflow-y-auto">
      <slot />
    </div>
  </aside>

  <BottomSheet
    v-if="isMobile"
    :open="open"
    :title="title"
    :aria-label="ariaLabel"
    panel-class="h-[75vh]"
    @close="emit('close')"
  >
    <slot />
  </BottomSheet>
</template>
