<!--
The grid of widgets. Role-neutral — the widget registry
(`widgets.ts`) filters entries by the current user's role, so this
component renders whatever set applies.

Drag UX:
  * Rect-containment hit-testing against frozen rects (via
    `usePointerSortable`). Cursor inside widget W targets W.
  * Preview reorder: the dragged widget's slot moves to the drop
    position as an accent-tinted placeholder; neighbours FLIP-glide
    to make room.
  * Drop commits `store.move` and FLIP animates the landing.
-->
<script setup lang="ts">
import { computed, toRef } from 'vue'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'
import { usePointerSortable } from '@/composables/usePointerSortable'
import {
  effectiveSpanFor,
  spanClass,
  widgetById,
} from './widgets'
import WidgetFrame from './WidgetFrame.vue'

const store = useDashboardLayoutStore()

// Grid gap grows in edit mode so corner controls on adjacent widgets
// don't crowd one another.
const gridGap = computed(() => (store.editMode ? 'gap-4' : 'gap-3'))

const { dragState, handlePointerDown, isDragged, previewOrder } = usePointerSortable({
  enabled: toRef(store, 'editMode'),
  onReorder: (from, to) => store.move(from, to),
})

const visibleEntries = computed(() =>
  store.layout.widgets
    .map((entry, originalIndex) => ({ entry, originalIndex }))
    .filter(({ entry }) => entry.visible && widgetById(entry.id)),
)

const rendered = computed(() => previewOrder(visibleEntries.value))
</script>

<template>
  <TransitionGroup
    tag="div"
    name="widget-flip"
    :class="[
      'grid grid-cols-1 xl:grid-cols-3 auto-rows-min transition-[gap] duration-150',
      gridGap,
      store.editMode && 'select-none',
      dragState.isDragging && 'cursor-grabbing',
    ]"
  >
    <WidgetFrame
      v-for="{ entry, originalIndex } in rendered"
      :key="entry.id"
      :index="originalIndex"
      :current-span="effectiveSpanFor(entry)"
      :edit-mode="store.editMode"
      :dragging="isDragged(originalIndex)"
      :component="widgetById(entry.id)!.component"
      :widget-props="widgetById(entry.id)!.props"
      :class="[
        spanClass(effectiveSpanFor(entry)),
        widgetById(entry.id)?.naturalHeight ? 'self-start' : '',
      ]"
      @hide="store.hide(entry.id)"
      @resize="(span) => store.setSpan(entry.id, span)"
      @handle-pointerdown="(e) => handlePointerDown(originalIndex, e)"
    />
  </TransitionGroup>
</template>

<style scoped>
.widget-flip-move {
  transition: transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
}
</style>
