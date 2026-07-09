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

/** Partial-span resize preview. `null` clears the preview. Fields
 *  omitted from a non-null payload keep their committed value. */
export interface ResizePreviewIntent {
  span?: WidgetSpan
  rowSpan?: WidgetSpan
}

export type MoveDirection = 'left' | 'right' | 'up' | 'down'

/** Which axes a resize handle drives. */
export type ResizeAxis = 'both' | 'x' | 'y'

export interface DashboardWidgetContext {
  editMode: Readonly<Ref<boolean>>
  dragging: Readonly<Ref<boolean>>
  currentSpan: Readonly<Ref<WidgetSpan>>
  currentRowSpan: Readonly<Ref<WidgetSpan>>
  /** Registry minimums; sizing UI disables options below these. */
  minSpan: WidgetSpan
  minRowSpan: WidgetSpan
  onResize: (span: WidgetSpan) => void
  onResizeRow: (rowSpan: WidgetSpan) => void
  /** Live sizing preview (context-menu hover). Writes nothing to the
   *  store; the grid re-packs around the previewed footprint until
   *  cleared with `null`. */
  onPreviewResize: (intent: ResizePreviewIntent | null) => void
  /** The resize context menu opened (`true`) or closed (`false`). Lets
   *  the grid freeze its height for the menu session so a preview can't
   *  jump the scroll. */
  onSizeMenuToggle: (open: boolean) => void
  /** Keyboard repositioning of the focused widget. */
  onMove: (dir: MoveDirection) => void
  onHide: () => void
  onHandlePointerDown: (e: PointerEvent) => void
}

export const DASHBOARD_WIDGET_CONTEXT: InjectionKey<DashboardWidgetContext> = Symbol(
  'DashboardWidgetContext',
)
