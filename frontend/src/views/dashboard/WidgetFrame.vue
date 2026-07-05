<!--
Positioning + context wrapper around a widget. Emits the
`data-sortable-index` that `usePointerSortable` uses for hit testing
and provides the per-widget `DashboardWidgetContext` so the widget's
`DashboardWidgetShell` can render edit-mode affordances inside its
own header.

Two rendering paths:

  * `frameWraps: false` (the default): the widget owns its own
    `DashboardWidgetShell`. The frame renders the component
    bare; this is the path every legacy widget takes and lets
    widgets that need rich shell state (loading / error / empty)
    drive the shell themselves.

  * `frameWraps: true`: the frame wraps the component in a shell
    whose only job is to carry the registry-defined `titleKey`.
    Used by simple chart widgets that delegate their state
    machine to the chart component itself (KpiTile, LineChart,
    etc) and just need a titled card around them. Removes ~12
    lines of boilerplate-per-widget that would otherwise have to
    exist as parallel wrapper components.

The widget is rendered via `<component :is>` directly inside this
template — NOT via a slot. Vue 3 `provide()` only reaches true
component-tree descendants, and slot content is a descendant of the
slot-writing parent (not the slot host), so using a slot here would
silently break the shell's `inject()` call.
-->
<script setup lang="ts">
import { provide, readonly, toRef, type Component } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  DASHBOARD_WIDGET_CONTEXT,
  type DashboardWidgetContext,
} from './widgetContext'
import type { WidgetSpan } from './widgets'
import DashboardWidgetShell from './DashboardWidgetShell.vue'

const props = defineProps<{
  index: number
  currentSpan: WidgetSpan
  editMode: boolean
  dragging: boolean
  /** Set briefly after an invalid drop on this source so the shell
   *  can run a single 180ms outline pulse confirming the drop was
   *  received. Drives a `.frame-pulse` keyframe in the scoped CSS;
   *  the grid clears the flag once the animation ends. */
  pulsing?: boolean
  /** The Vue component to render inside the frame. */
  component: Component
  /** Static props forwarded to the rendered widget component. */
  widgetProps?: Record<string, unknown>
  /** When `true`, the frame wraps the component in a `DashboardWidgetShell`
   *  with the title resolved from `frameTitleKey`. */
  frameWraps?: boolean
  /** Fluent key for the title rendered in the frame-supplied shell.
   *  Required when `frameWraps` is `true`. */
  frameTitleKey?: string
  /** CSS `aspect-ratio` for a plotted-chart body, forwarded to the
   *  frame-supplied shell. See `DashboardWidgetShell.bodyAspect`. */
  bodyAspect?: string
}>()

const emit = defineEmits<{
  (e: 'hide'): void
  (e: 'resize', span: WidgetSpan): void
  (e: 'handle-pointerdown', ev: PointerEvent): void
  /** Corner-resize grip pressed. The grid owns the gesture (it has the
   *  lattice metrics) and resizes this widget's col/row span live. */
  (e: 'resize-pointerdown', ev: PointerEvent): void
}>()

const fluent = useFluent()

const context: DashboardWidgetContext = {
  editMode: readonly(toRef(props, 'editMode')),
  dragging: readonly(toRef(props, 'dragging')),
  currentSpan: readonly(toRef(props, 'currentSpan')),
  onHide: () => emit('hide'),
  onResize: (span) => emit('resize', span),
  onHandlePointerDown: (e) => emit('handle-pointerdown', e),
}

provide(DASHBOARD_WIDGET_CONTEXT, context)
</script>

<template>
  <div
    :data-sortable-index="index"
    :class="['relative', 'min-w-0', 'min-h-0', 'h-full', pulsing ? 'frame-pulse' : '']"
  >
    <DashboardWidgetShell
      v-if="frameWraps && frameTitleKey"
      :title="fluent.$t(frameTitleKey)"
      :body-aspect="bodyAspect"
    >
      <component :is="component" v-bind="widgetProps ?? {}" />
    </DashboardWidgetShell>
    <component v-else :is="component" v-bind="widgetProps ?? {}" />

    <!-- Corner-resize grip. Edit-mode only, and only at xl where the
         grid is multi-column (below xl every widget is full-width, so
         column resize is meaningless). The grid handles the gesture. -->
    <button
      v-if="editMode"
      type="button"
      class="absolute bottom-0.5 right-0.5 z-20 hidden xl:flex h-4 w-4 items-end justify-end cursor-nwse-resize touch-none text-tertiary hover:text-accent focus-visible:text-accent focus-visible:outline-none"
      aria-label="Resize widget"
      @pointerdown="(e) => emit('resize-pointerdown', e)"
    >
      <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
        <path d="M9 1 L1 9 M9 5 L5 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
      </svg>
    </button>
  </div>
</template>

<style scoped>
/* Invalid-drop confirmation. Outline glows once at accent intensity
 * over 180ms, then fades to zero. No motion (color only) so the
 * pulse remains under prefers-reduced-motion too. The animation runs
 * on the sortable-indexed root, so it works whether the widget owns
 * its own shell or uses the frame-supplied one. */
.frame-pulse {
  animation: frame-pulse 180ms ease-out;
  border-radius: 0.75rem;
}

@keyframes frame-pulse {
  0% {
    outline: 2px solid color-mix(in srgb, var(--color-accent) 0%, transparent);
    outline-offset: 2px;
  }
  40% {
    outline: 2px solid color-mix(in srgb, var(--color-accent) 70%, transparent);
    outline-offset: 2px;
  }
  100% {
    outline: 2px solid color-mix(in srgb, var(--color-accent) 0%, transparent);
    outline-offset: 2px;
  }
}
</style>
