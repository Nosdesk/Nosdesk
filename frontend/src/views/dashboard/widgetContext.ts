/**
 * Typed provide/inject contract between the dashboard parent (which
 * owns the drag state + mutator callbacks) and each widget's
 * `DashboardWidgetShell` (which renders edit-mode affordances).
 *
 * Widgets never see this — they just use `<DashboardWidgetShell
 * title="…">` and the shell picks the context up automatically. When
 * a widget is rendered outside the dashboard, `inject()` returns
 * `undefined` and the shell omits all edit affordances.
 */
import type { InjectionKey, Ref } from 'vue'
import type { WidgetSpan } from './widgets'

export interface DashboardWidgetContext {
  editMode: Readonly<Ref<boolean>>
  dragging: Readonly<Ref<boolean>>
  currentSpan: Readonly<Ref<WidgetSpan>>
  onResize: (span: WidgetSpan) => void
  onHide: () => void
  onHandlePointerDown: (e: PointerEvent) => void
}

export const DASHBOARD_WIDGET_CONTEXT: InjectionKey<DashboardWidgetContext> = Symbol(
  'DashboardWidgetContext',
)
