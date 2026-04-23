<!--
Positioning + context wrapper around a widget. Emits the
`data-sortable-index` that `usePointerSortable` uses for hit testing
and provides the per-widget `DashboardWidgetContext` so the widget's
`DashboardWidgetShell` can render edit-mode affordances inside its
own header.

The widget is rendered via `<component :is>` directly inside this
template — NOT via a slot. Vue 3 `provide()` only reaches true
component-tree descendants, and slot content is a descendant of the
slot-writing parent (not the slot host), so using a slot here would
silently break the shell's `inject()` call.
-->
<script setup lang="ts">
import { provide, readonly, toRef, type Component } from 'vue'
import {
  DASHBOARD_WIDGET_CONTEXT,
  type DashboardWidgetContext,
} from './widgetContext'
import type { WidgetSpan } from './widgets'

const props = defineProps<{
  index: number
  currentSpan: WidgetSpan
  editMode: boolean
  dragging: boolean
  /** The Vue component to render inside the frame. */
  component: Component
  /** Static props forwarded to the rendered widget component. */
  widgetProps?: Record<string, unknown>
}>()

const emit = defineEmits<{
  (e: 'hide'): void
  (e: 'resize', span: WidgetSpan): void
  (e: 'handle-pointerdown', ev: PointerEvent): void
}>()

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
    <component :is="component" v-bind="widgetProps ?? {}" />
  </div>
</template>
