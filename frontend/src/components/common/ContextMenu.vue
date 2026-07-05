<script setup lang="ts">
/**
 * Right-click / "more actions" menu, anchored to a click point.
 * Public API unchanged from prior implementations; positioning
 * + dismiss + focus delegate to `<ResponsiveMenu>` (anchored
 * popover on desktop, bottom sheet on mobile), item rendering
 * delegates to `<MenuList>`. This component is a thin connector
 * that picks "click-anchored surface wraps menu list".
 */
import { computed } from 'vue'
import ResponsiveMenu from './ResponsiveMenu.vue'
import MenuList, { type MenuItem } from './MenuList.vue'

// Re-export so existing `import type { MenuItem } from '.../ContextMenu.vue'`
// call sites keep working without churn.
export type { MenuItem }

const props = defineProps<{
  items: MenuItem[]
  x: number
  y: number
  /** Open state. The component is always mounted; toggling
   * `open` is what plays Popover's enter/leave transition. v-if
   * mounting the entire ContextMenu would unmount the inner
   * Transition before the leave can run, so it's not supported. */
  open: boolean
}>()

const emit = defineEmits<{
  select: [id: string]
  close: []
}>()

const handleSelect = (id: string) => {
  emit('select', id)
  emit('close')
}

const anchor = computed(() => ({
  type: 'point' as const,
  x: props.x,
  y: props.y,
}))
</script>

<template>
  <ResponsiveMenu
    :open="open"
    :anchor="anchor"
    placement="bottom-start"
    react-to-scroll="close"
    role="menu"
    popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[160px]"
    @close="emit('close')"
  >
    <MenuList :items="items" @select="handleSelect" />
  </ResponsiveMenu>
</template>
