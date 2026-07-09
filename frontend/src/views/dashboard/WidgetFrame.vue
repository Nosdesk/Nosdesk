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

Resize affordances (edit mode, xl+ only; below xl the grid is a
single column and pointer resize is meaningless):

  * Right-edge handle: width only (cursor ew-resize).
  * Bottom-edge handle: height only (cursor ns-resize).
  * SE corner grip: both axes (cursor nwse-resize). Sits above the
    edge handles (z-20 vs z-10) so it wins where they overlap.

The handles are 12px invisible hit zones straddling the card border;
each reveals a small accent bar when the frame is hovered. The grid
owns the gesture (it has the lattice metrics); the frame only emits
`resize-pointerdown` with the axis.
-->
<script setup lang="ts">
import { provide, readonly, toRef, type Component } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  DASHBOARD_WIDGET_CONTEXT,
  type DashboardWidgetContext,
  type MoveDirection,
  type ResizeAxis,
  type ResizePreviewIntent,
} from './widgetContext'
import type { WidgetSpan } from './widgets'
import DashboardWidgetShell from './DashboardWidgetShell.vue'

const props = defineProps<{
  index: number
  currentSpan: WidgetSpan
  currentRowSpan: WidgetSpan
  /** Registry minimum spans; sizing UI disables options below these. */
  minSpan: WidgetSpan
  minRowSpan: WidgetSpan
  editMode: boolean
  dragging: boolean
  /** Set briefly after an invalid drop on this source so the shell
   *  can run a single 180ms outline pulse confirming the drop was
   *  received. Drives a `.frame-pulse` keyframe in the scoped CSS;
   *  the grid clears the flag once the animation ends. */
  pulsing?: boolean
  /** Non-null while a resize gesture is reshaping this widget: the
   *  formatted "cols × rows" readout rendered as a centered chip. */
  sizeBadge?: string | null
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
  (e: 'resize-row', rowSpan: WidgetSpan): void
  (e: 'preview-resize', intent: ResizePreviewIntent | null): void
  (e: 'size-menu-toggle', open: boolean): void
  (e: 'move', dir: MoveDirection): void
  (e: 'handle-pointerdown', ev: PointerEvent): void
  /** A resize handle was pressed. The grid owns the gesture (it has
   *  the lattice metrics) and resizes this widget's col / row span
   *  live along the given axis. */
  (e: 'resize-pointerdown', ev: PointerEvent, axis: ResizeAxis): void
}>()

const fluent = useFluent()

const context: DashboardWidgetContext = {
  editMode: readonly(toRef(props, 'editMode')),
  dragging: readonly(toRef(props, 'dragging')),
  currentSpan: readonly(toRef(props, 'currentSpan')),
  currentRowSpan: readonly(toRef(props, 'currentRowSpan')),
  minSpan: props.minSpan,
  minRowSpan: props.minRowSpan,
  onResize: (span) => emit('resize', span),
  onResizeRow: (rowSpan) => emit('resize-row', rowSpan),
  onPreviewResize: (intent) => emit('preview-resize', intent),
  onSizeMenuToggle: (open) => emit('size-menu-toggle', open),
  onMove: (dir) => emit('move', dir),
  onHide: () => emit('hide'),
  onHandlePointerDown: (e) => emit('handle-pointerdown', e),
}

provide(DASHBOARD_WIDGET_CONTEXT, context)
</script>

<template>
  <div
    :data-sortable-index="index"
    :class="[
      'group/frame relative min-w-0 min-h-0 h-full',
      pulsing ? 'frame-pulse' : '',
    ]"
  >
    <DashboardWidgetShell
      v-if="frameWraps && frameTitleKey"
      :title="fluent.$t(frameTitleKey)"
      :body-aspect="bodyAspect"
    >
      <component :is="component" v-bind="widgetProps ?? {}" />
    </DashboardWidgetShell>
    <component v-else :is="component" v-bind="widgetProps ?? {}" />

    <template v-if="editMode">
      <!-- Right-edge handle: width only. Invisible 12px hit zone
           straddling the border; the inner accent bar appears on
           frame hover so the chrome stays quiet at rest. -->
      <button
        type="button"
        class="absolute top-3 bottom-3 -right-1.5 z-10 hidden xl:flex w-3 items-center justify-center cursor-ew-resize touch-none focus-visible:outline-none"
        :aria-label="fluent.$t('dashboard-widget-resize-width-label')"
        @pointerdown="(e) => emit('resize-pointerdown', e, 'x')"
      >
        <span
          class="w-[3px] h-8 rounded-full bg-accent/60 opacity-0 transition-opacity group-hover/frame:opacity-60 group-focus-within/frame:opacity-60 hover:!opacity-100"
          aria-hidden="true"
        />
      </button>

      <!-- Bottom-edge handle: height only. -->
      <button
        type="button"
        class="absolute left-3 right-3 -bottom-1.5 z-10 hidden xl:flex h-3 items-center justify-center cursor-ns-resize touch-none focus-visible:outline-none"
        :aria-label="fluent.$t('dashboard-widget-resize-height-label')"
        @pointerdown="(e) => emit('resize-pointerdown', e, 'y')"
      >
        <span
          class="h-[3px] w-8 rounded-full bg-accent/60 opacity-0 transition-opacity group-hover/frame:opacity-60 group-focus-within/frame:opacity-60 hover:!opacity-100"
          aria-hidden="true"
        />
      </button>

      <!-- SE corner grip: both axes. Above the edge handles so it
           wins where the hit zones overlap. -->
      <button
        type="button"
        class="absolute bottom-0.5 right-0.5 z-20 hidden xl:flex h-4 w-4 items-end justify-end cursor-nwse-resize touch-none text-tertiary hover:text-accent focus-visible:text-accent focus-visible:outline-none"
        :aria-label="fluent.$t('dashboard-widget-resize-corner-label')"
        @pointerdown="(e) => emit('resize-pointerdown', e, 'both')"
      >
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
          <path d="M9 1 L1 9 M9 5 L5 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
        </svg>
      </button>

      <!-- Live size readout while a resize gesture reshapes this
           widget. Centered chip, no pointer interference. -->
      <div
        v-if="sizeBadge"
        class="absolute inset-0 z-30 flex items-center justify-center pointer-events-none"
        aria-hidden="true"
      >
        <span
          class="px-2 py-1 rounded-md bg-surface/90 border border-default shadow text-xs font-medium tabular-nums text-primary"
        >
          {{ sizeBadge }}
        </span>
      </div>
    </template>
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
