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
import { computed, ref, toRef, watch } from 'vue'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'
import {
  projectedTargetIndex,
  usePointerSortable,
  walkFlow,
  type ProjectableEntry,
} from '@/composables/usePointerSortable'
import {
  effectiveSpanFor,
  spanClass,
  widgetById,
} from './widgets'
import WidgetFrame from './WidgetFrame.vue'

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
  })),
)

// Layout snapshot captured at drag-start. The pixel deltas needed
// for transforms are computed from this; no per-cursor-move DOM
// measurement runs.
interface LayoutSnapshot {
  colWidth: number
  colGap: number
  rowGap: number
  rowHeights: number[]
}

const layoutSnapshot = ref<LayoutSnapshot | null>(null)

function captureLayoutSnapshot(): LayoutSnapshot | null {
  const grid = gridEl.value
  if (!grid) return null
  const gridRect = grid.getBoundingClientRect()
  const style = getComputedStyle(grid)
  const colGap = parseFloat(style.columnGap || style.gap || '0') || 0
  const rowGap = parseFloat(style.rowGap || style.gap || '0') || 0
  const cols = Math.max(1, dragState.renderedColumns)
  const colWidth = (gridRect.width - (cols - 1) * colGap) / cols

  interface RowBounds { top: number; bottom: number }
  const rows: RowBounds[] = []
  const tolerance = 4
  for (const child of Array.from(grid.children) as HTMLElement[]) {
    if (!child.hasAttribute('data-sortable-index')) continue
    const r = child.getBoundingClientRect()
    const existing = rows.find((row) => Math.abs(row.top - r.top) < tolerance)
    if (existing) {
      existing.bottom = Math.max(existing.bottom, r.bottom)
    } else {
      rows.push({ top: r.top, bottom: r.bottom })
    }
  }
  rows.sort((a, b) => a.top - b.top)
  return {
    colWidth,
    colGap,
    rowGap,
    rowHeights: rows.map((r) => r.bottom - r.top),
  }
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
  const origCells = walkFlow(entries, cols)
  const projCells = walkFlow(projected, cols)

  const rowDelta = (fromRow: number, toRow: number): number => {
    if (fromRow === toRow) return 0
    const dir = toRow > fromRow ? 1 : -1
    let dy = 0
    const lo = Math.min(fromRow, toRow)
    const hi = Math.max(fromRow, toRow)
    for (let r = lo; r < hi; r++) {
      const h = snap.rowHeights[r] ?? snap.rowHeights[snap.rowHeights.length - 1] ?? 0
      dy += h + snap.rowGap
    }
    return dy * dir
  }

  for (const e of entries) {
    const orig = origCells.get(e.originalIndex)
    const proj = projCells.get(e.originalIndex)
    if (!orig || !proj) continue
    if (orig.row === proj.row && orig.col === proj.col) continue
    const dx = (proj.col - orig.col) * (snap.colWidth + snap.colGap)
    const dy = rowDelta(orig.row, proj.row)
    map.set(e.originalIndex, `translate(${dx}px, ${dy}px)`)
  }
  return map
})

function styleFor(originalIndex: number) {
  const transform = transformMap.value.get(originalIndex)
  if (transform) return { transform }
  return undefined
}
</script>

<template>
  <div
    ref="gridEl"
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
        'widget-projected',
      ]"
      :style="styleFor(originalIndex)"
      @hide="store.hide(entry.id)"
      @resize="(span) => store.setSpan(entry.id, span)"
      @handle-pointerdown="(e) => handlePointerDown(originalIndex, e)"
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
