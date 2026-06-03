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
}>()

const emit = defineEmits<{
  (e: 'hide'): void
  (e: 'resize', span: WidgetSpan): void
  (e: 'handle-pointerdown', ev: PointerEvent): void
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
  <div :data-sortable-index="index">
    <DashboardWidgetShell
      v-if="frameWraps && frameTitleKey"
      :title="fluent.$t(frameTitleKey)"
    >
      <component :is="component" v-bind="widgetProps ?? {}" />
    </DashboardWidgetShell>
    <component v-else :is="component" v-bind="widgetProps ?? {}" />
  </div>
</template>
