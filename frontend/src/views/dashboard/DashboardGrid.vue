<!--
The grid of widgets. Role-neutral — the widget registry
(`widgets.ts`) filters entries by the current user's role, so this
component renders whatever set applies.

Drag UX (post-2026-06 redesign per docs/plans/v1-dashboard-dnd-spec.md):
  * Source widget stays in place at reduced content opacity with an
    accent outline. No floating clone, no placeholder swap.
  * Siblings stay frozen at their drag-start positions; the rendered
    order is `visibleEntries` directly, NOT a preview reorder.
  * A `DropIndicator` line marks the projected post-drop slot,
    computed via `computeDropTargetGap` (an in-memory splice + flow-
    column linear pass, same operation the store will run on commit).
  * Commit runs once on pointerup. The existing `widget-flip-move`
    FLIP transition animates displaced siblings into their new
    positions. Honours `prefers-reduced-motion` via a CSS gate.
  * Invalid drop (no movement OR cursor over no valid target) fires
    a soft outline pulse on the source via `pulse-source` for 180ms.
-->
<script setup lang="ts">
import { computed, onMounted, ref, toRef, watch } from 'vue'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'
import {
  computeDropTargetGap,
  usePointerSortable,
  type ProjectableEntry,
} from '@/composables/usePointerSortable'
import {
  effectiveSpanFor,
  spanClass,
  widgetById,
} from './widgets'
import WidgetFrame from './WidgetFrame.vue'
import DropIndicator from './DropIndicator.vue'

const store = useDashboardLayoutStore()

// Grid gap grows in edit mode so corner controls on adjacent widgets
// don't crowd one another.
const gridGap = computed(() => (store.editMode ? 'gap-4' : 'gap-3'))

// TransitionGroup's ref resolves to the component instance, not the
// underlying DOM element; the actual <div> sits on `$el`. We expose a
// derived `gridEl` (the real HTMLElement) for `getGridEl` and
// DropIndicator's position math, and update it on mount.
const transitionGroupRef = ref<{ $el?: HTMLElement } | null>(null)
const gridEl = ref<HTMLElement | null>(null)
function syncGridEl() {
  gridEl.value = transitionGroupRef.value?.$el ?? null
}

// Originalindex-keyed transient flag for the invalid-drop outline
// pulse. Cleared 180ms after activation; the WidgetFrame reads it
// and applies a CSS class.
const pulseSourceIndex = ref<number | null>(null)
let pulseTimer: ReturnType<typeof setTimeout> | null = null

function flashSourcePulse(sourceIndex: number) {
  if (pulseTimer) clearTimeout(pulseTimer)
  pulseSourceIndex.value = sourceIndex
  pulseTimer = setTimeout(() => {
    pulseSourceIndex.value = null
    pulseTimer = null
  }, 180)
}

const { dragState, handlePointerDown, isDragged } = usePointerSortable({
  enabled: toRef(store, 'editMode'),
  onReorder: (from, to) => store.move(from, to),
  onInvalidDrop: flashSourcePulse,
  getGridEl: () => gridEl.value,
})

onMounted(syncGridEl)
watch(transitionGroupRef, syncGridEl)

const visibleEntries = computed(() =>
  store.layout.widgets
    .map((entry, originalIndex) => ({ entry, originalIndex }))
    .filter(({ entry }) => entry.visible && widgetById(entry.id)),
)

/** Visible entries projected to the shape the drop-position helper
 *  needs (original index + effective col span). Recomputed reactively
 *  so resizing or hide / unhide doesn't desync the projection. */
const projectableEntries = computed<ProjectableEntry[]>(() =>
  visibleEntries.value.map(({ entry, originalIndex }) => ({
    originalIndex,
    colSpan: effectiveSpanFor(entry),
  })),
)

/** The projected post-drop slot the DropIndicator renders against.
 *  Null when not dragging or when no meaningful projection exists
 *  (source === hover, or hover not yet set). */
const dropTargetGap = computed(() => {
  if (!dragState.isDragging) return null
  return computeDropTargetGap(
    projectableEntries.value,
    dragState.sourceIndex,
    dragState.hoverIndex,
    dragState.dropBefore,
    dragState.renderedColumns,
  )
})
</script>

<template>
  <div class="relative">
    <!-- The ref lives on the TransitionGroup so its root <div> (the
         element with `display: grid`) is what `getGridEl` returns.
         TransitionGroup exposes the root via `$el` on the instance. -->
    <TransitionGroup
      ref="transitionGroupRef"
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
        v-for="{ entry, originalIndex } in visibleEntries"
        :key="entry.id"
        :index="originalIndex"
        :current-span="effectiveSpanFor(entry)"
        :edit-mode="store.editMode"
        :dragging="isDragged(originalIndex)"
        :pulsing="pulseSourceIndex === originalIndex"
        :component="widgetById(entry.id)!.component"
        :widget-props="widgetById(entry.id)!.props"
        :frame-wraps="widgetById(entry.id)?.frameWraps ?? false"
        :frame-title-key="widgetById(entry.id)?.titleKey"
        :class="[
          spanClass(effectiveSpanFor(entry)),
          widgetById(entry.id)?.naturalHeight ? 'self-start' : '',
        ]"
        @hide="store.hide(entry.id)"
        @resize="(span) => store.setSpan(entry.id, span)"
        @handle-pointerdown="(e) => handlePointerDown(originalIndex, e)"
      />
    </TransitionGroup>

    <DropIndicator
      v-if="dragState.isDragging && dropTargetGap"
      :gap="dropTargetGap"
      :grid-el="gridEl"
      :rendered-columns="dragState.renderedColumns"
    />
  </div>
</template>

<style scoped>
.widget-flip-move {
  transition: transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

/* Reduced-motion users get instant cuts on the post-drop reflow.
 * The indicator itself has no transition either (state readout,
 * not animation), so this is the only motion-gate the grid needs. */
@media (prefers-reduced-motion: reduce) {
  .widget-flip-move {
    transition: none;
  }
}
</style>
