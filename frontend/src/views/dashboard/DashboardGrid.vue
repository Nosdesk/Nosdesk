<!--
The grid of widgets. Role-neutral, the widget registry
(`widgets.ts`) filters entries by the current user's role, so this
component renders whatever set applies.

Drag UX — "stable DOM, projected transforms":

  * The DOM order matches `visibleEntries` for the lifetime of the
    drag. Nothing in the v-for moves. Pointer capture is unaffected;
    cursor events flow exactly as they would if nothing was being
    dragged.

  * On drag-start we snapshot the grid's pixel layout (column width,
    row gaps, per-row heights). The engine emits intent
    (`sourceIndex` / `hoverIndex` / `dropBefore`) and from that we
    derive a `Map<originalIndex, "translate(dx, dy)">` that places
    every widget at its projected post-commit cell.

  * Each widget renders with `:style="{ transform: ... }"` and a CSS
    transition on `transform`. Crossing a snap zone updates the map;
    every displaced widget slides to its new projected slot in 180ms.
    The source widget transforms to its destination too — combined
    with the shell's dragging styling (dashed accent outline + 40%
    body opacity) it IS the magnet zone.

  * Commit runs once on pointerup. The engine calls `store.move`;
    `visibleEntries` re-emits in post-commit order. Because the
    pre-commit visual layout (DOM in original order + projected
    transforms) is pixel-identical to the post-commit visual layout
    (DOM in projected order + no transforms), there is no jump.

  * Invalid drop fires a soft outline pulse on the source.
-->
<script setup lang="ts">
import { computed, onBeforeUnmount, ref, toRef, watch } from 'vue'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'
import {
  packGrid,
  projectedTargetIndex,
  snapshotGridColumnCount,
  usePointerSortable,
  type ProjectableEntry,
} from '@/composables/usePointerSortable'
import {
  effectiveSpanFor,
  rowSpanClass,
  rowSpanFor,
  spanClass,
  widgetById,
  type WidgetSpan,
} from './widgets'
import WidgetFrame from './WidgetFrame.vue'

type LayoutEntry = { id: string; span?: WidgetSpan; rowSpan?: WidgetSpan }

const store = useDashboardLayoutStore()

const gridGap = computed(() => (store.editMode ? 'gap-4' : 'gap-3'))

const gridEl = ref<HTMLElement | null>(null)

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

const visibleEntries = computed(() =>
  store.layout.widgets
    .map((entry, originalIndex) => ({ entry, originalIndex }))
    .filter(({ entry }) => entry.visible && widgetById(entry.id)),
)

const projectableEntries = computed<ProjectableEntry[]>(() =>
  visibleEntries.value.map(({ entry, originalIndex }) => ({
    originalIndex,
    colSpan: effectiveSpanFor(entry),
    rowSpan: rowSpanFor(entry),
  })),
)

// Lattice metrics captured at drag-start. Because the grid is a
// fixed-unit lattice (`grid-auto-rows: var(--dash-row-unit)`), a cell
// delta maps to a constant pixel offset — no per-row content
// measurement, so the projected transforms can't drift from the real
// post-drop layout the way the old `auto-rows-min` snapshot did.
interface LayoutSnapshot {
  colWidth: number
  colGap: number
  rowGap: number
  rowUnit: number
}

const layoutSnapshot = ref<LayoutSnapshot | null>(null)

function captureLayoutSnapshot(): LayoutSnapshot | null {
  const grid = gridEl.value
  if (!grid) return null
  const gridRect = grid.getBoundingClientRect()
  const style = getComputedStyle(grid)
  const colGap = parseFloat(style.columnGap || style.gap || '0') || 0
  const rowGap = parseFloat(style.rowGap || style.gap || '0') || 0
  const cols = Math.max(1, snapshotGridColumnCount(grid))
  const colWidth = (gridRect.width - (cols - 1) * colGap) / cols
  // The lattice row height comes straight from the resolved
  // `grid-auto-rows` track, which is the same for every row.
  const rowUnit = parseFloat(style.gridAutoRows) || 0
  return { colWidth, colGap, rowGap, rowUnit }
}

watch(
  () => dragState.isDragging,
  (active) => {
    layoutSnapshot.value = active ? captureLayoutSnapshot() : null
  },
)

const transformMap = computed<Map<number, string>>(() => {
  const map = new Map<number, string>()
  if (!dragState.isDragging) return map
  const snap = layoutSnapshot.value
  if (!snap) return map
  const entries = projectableEntries.value
  const to = projectedTargetIndex(
    entries,
    dragState.sourceIndex,
    dragState.hoverIndex,
    dragState.dropBefore,
  )
  if (to === null) return map
  const srcV = entries.findIndex((e) => e.originalIndex === dragState.sourceIndex)
  if (srcV === -1) return map

  const projected = entries.slice()
  const [moved] = projected.splice(srcV, 1)
  projected.splice(to, 0, moved)

  const cols = Math.max(1, dragState.renderedColumns)
  const origCells = packGrid(entries, cols)
  const projCells = packGrid(projected, cols)

  // Uniform lattice: a one-cell step is a constant pixel distance on
  // each axis (track size + gap), so the transform exactly matches
  // where the widget lands after the real reflow on drop.
  const colPitch = snap.colWidth + snap.colGap
  const rowPitch = snap.rowUnit + snap.rowGap

  for (const e of entries) {
    const orig = origCells.get(e.originalIndex)
    const proj = projCells.get(e.originalIndex)
    if (!orig || !proj) continue
    if (orig.row === proj.row && orig.col === proj.col) continue
    const dx = (proj.col - orig.col) * colPitch
    const dy = (proj.row - orig.row) * rowPitch
    map.set(e.originalIndex, `translate(${dx}px, ${dy}px)`)
  }
  return map
})

function styleFor(originalIndex: number) {
  const transform = transformMap.value.get(originalIndex)
  if (transform) return { transform }
  return undefined
}

// The lattice row unit, shared by the grid track (`--dash-row-unit`) and the
// mobile max-height cap so the two never drift.
const DASH_ROW_UNIT_REM = 8.5

// Auto-rows behaviour. In edit mode the fixed lattice holds at every breakpoint
// so the drag projection (which reads the fixed row unit) stays exact. In view
// mode the 1-column mobile layout uses `auto` rows, so widgets size to their
// content instead of a fixed span; xl keeps the lattice.
const gridAutoRows = computed(() =>
  store.editMode
    ? '[grid-auto-rows:var(--dash-row-unit)]'
    : '[grid-auto-rows:auto] xl:[grid-auto-rows:var(--dash-row-unit)]',
)

// Per-widget inline style: the drag transform (when dragging) plus, in view
// mode, the mobile max-height cap (rowSpan × row unit). `max-h-[var(--dash-max-h)]`
// on the frame reads this; it's `xl:max-h-none` so desktop keeps the fixed span.
function widgetStyle(entry: LayoutEntry, originalIndex: number) {
  const styles: Array<Record<string, string> | undefined> = [styleFor(originalIndex)]
  if (!store.editMode) {
    styles.push({ '--dash-max-h': `${rowSpanOf(entry) * DASH_ROW_UNIT_REM}rem` })
  }
  return styles
}

// -- Corner resize -----------------------------------------------------
//
// The grip on each widget (WidgetFrame) drives a live re-pack: as the
// pointer drags, the widget's column / row span follow it (snapped to
// lattice cells) and the grid reflows reactively via `colSpanOf` /
// `rowSpanOf`. The change is committed to the store once on pointerup,
// so the whole gesture is a single undo step rather than one per frame.
const resizePreview = ref<{ id: string; span: WidgetSpan; rowSpan: WidgetSpan } | null>(null)
let resizeStart: {
  id: string
  left: number
  top: number
  colPitch: number
  rowPitch: number
  cols: number
} | null = null
let resizePointerId = -1

/** Rendered column span, preview-aware so the resized widget reflows
 *  live without committing per frame. */
function colSpanOf(entry: LayoutEntry): WidgetSpan {
  if (resizePreview.value?.id === entry.id) return resizePreview.value.span
  return effectiveSpanFor(entry)
}
function rowSpanOf(entry: LayoutEntry): WidgetSpan {
  if (resizePreview.value?.id === entry.id) return resizePreview.value.rowSpan
  return rowSpanFor(entry)
}

const clampSpan = (n: number): WidgetSpan => Math.max(1, Math.min(3, n)) as WidgetSpan

function onResizePointerDown(originalIndex: number, e: PointerEvent) {
  if (!store.editMode) return
  if (e.button !== undefined && e.button !== 0) return
  e.preventDefault()
  e.stopPropagation()
  const entry = visibleEntries.value.find((v) => v.originalIndex === originalIndex)?.entry
  const snap = captureLayoutSnapshot()
  const el = gridEl.value?.querySelector<HTMLElement>(`[data-sortable-index="${originalIndex}"]`)
  if (!entry || !snap || !el) return

  const rect = el.getBoundingClientRect()
  resizeStart = {
    id: entry.id,
    left: rect.left,
    top: rect.top,
    colPitch: snap.colWidth + snap.colGap,
    rowPitch: snap.rowUnit + snap.rowGap,
    cols: Math.max(1, snapshotGridColumnCount(gridEl.value)),
  }
  resizePreview.value = { id: entry.id, span: effectiveSpanFor(entry), rowSpan: rowSpanFor(entry) }
  resizePointerId = e.pointerId
  try {
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
  } catch {
    // capture is best-effort; the document listeners still track the drag.
  }
  document.addEventListener('pointermove', onResizeMove)
  document.addEventListener('pointerup', onResizeUp)
  document.addEventListener('pointercancel', onResizeCancel)
}

function onResizeMove(e: PointerEvent) {
  if (!resizeStart || e.pointerId !== resizePointerId) return
  // Span counted from the widget's drag-start top-left, snapped to the
  // nearest cell and clamped: columns to what the grid actually has,
  // rows to the 1-3 range the lattice + backend allow.
  const rawCols = Math.round((e.clientX - resizeStart.left) / resizeStart.colPitch)
  const span = Math.max(1, Math.min(resizeStart.cols, rawCols)) as WidgetSpan
  const rowSpan = clampSpan(Math.round((e.clientY - resizeStart.top) / resizeStart.rowPitch))
  resizePreview.value = { id: resizeStart.id, span, rowSpan }
}

function endResize() {
  document.removeEventListener('pointermove', onResizeMove)
  document.removeEventListener('pointerup', onResizeUp)
  document.removeEventListener('pointercancel', onResizeCancel)
  resizeStart = null
  resizePointerId = -1
}

function onResizeUp(e: PointerEvent) {
  if (e.pointerId !== resizePointerId) return
  const preview = resizePreview.value
  const start = resizeStart
  endResize()
  resizePreview.value = null
  if (preview && start) {
    store.setSpan(start.id, preview.span)
    store.setRowSpan(start.id, preview.rowSpan)
  }
}

function onResizeCancel() {
  endResize()
  resizePreview.value = null
}

onBeforeUnmount(endResize)
</script>

<template>
  <div
    ref="gridEl"
    :class="[
      'grid grid-cols-1 xl:grid-cols-3 [grid-auto-flow:row_dense] transition-[gap] duration-150',
      gridAutoRows,
      gridGap,
      store.editMode && 'select-none',
      dragState.isDragging && 'cursor-grabbing',
    ]"
    :style="{ '--dash-row-unit': '8.5rem' }"
  >
    <WidgetFrame
      v-for="{ entry, originalIndex } in visibleEntries"
      :key="entry.id"
      :index="originalIndex"
      :current-span="colSpanOf(entry)"
      :edit-mode="store.editMode"
      :dragging="isDragged(originalIndex)"
      :pulsing="pulseSourceIndex === originalIndex"
      :component="widgetById(entry.id)!.component"
      :widget-props="widgetById(entry.id)!.props"
      :frame-wraps="widgetById(entry.id)?.frameWraps ?? false"
      :frame-title-key="widgetById(entry.id)?.titleKey"
      :body-aspect="widgetById(entry.id)?.bodyAspect"
      :class="[
        spanClass(colSpanOf(entry)),
        rowSpanClass(rowSpanOf(entry), store.editMode),
        !store.editMode && 'max-h-[var(--dash-max-h)] xl:max-h-none',
        'widget-projected',
      ]"
      :style="widgetStyle(entry, originalIndex)"
      @hide="store.hide(entry.id)"
      @resize="(span) => store.setSpan(entry.id, span)"
      @handle-pointerdown="(e) => handlePointerDown(originalIndex, e)"
      @resize-pointerdown="(e) => onResizePointerDown(originalIndex, e)"
    />
  </div>
</template>

<style scoped>
/* Every widget animates its own transform. During a drag the
 * transform map updates on every snap-zone change; without this
 * transition the projected layout would snap instead of slide. */
.widget-projected {
  transition: transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

@media (prefers-reduced-motion: reduce) {
  .widget-projected {
    transition: none;
  }
}
</style>
