<script setup lang="ts">
/**
 * Right-click / "more actions" menu, anchored to a click point.
 * Public API unchanged from prior implementations; positioning
 * + dismiss + focus delegate to `<Popover>`, item rendering
 * delegates to `<MenuList>`. This component is now a 30-line
 * connector that picks "click-anchored popover wraps menu list".
 *
 * Element-anchored equivalents (e.g. `DocumentActionsMenu`) use
 * the same `<MenuList>` inside a `<Popover>` with an element
 * anchor — same chrome, different anchor mode.
 */
import { computed } from 'vue'
import Popover from './Popover.vue'
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
  <Popover
    :open="open"
    :anchor="anchor"
    placement="bottom-start"
    react-to-scroll="close"
    role="menu"
    popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[160px]"
    @close="emit('close')"
  >
    <MenuList :items="items" @select="handleSelect" />
  </Popover>
</template>
