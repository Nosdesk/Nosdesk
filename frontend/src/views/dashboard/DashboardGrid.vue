<!--
The grid of widgets. Role-neutral, the widget registry
(`widgets.ts`) filters entries by the current user's role, so this
component renders whatever set applies.

Placement model, "column anchors + gravity":

  * The stored layout carries list order (vertical intent) plus an
    anchor column per widget. Rows are always derived by the gravity
    packer (`packAnchored`): each widget floats up within its column
    band, collisions push down. This is the react-grid-layout /
    gridstack compaction model; it lets a drag honor the cursor's
    column exactly (including the right column with empty cells
    beside it) while layouts stay tidy without persisting rows.

  * At xl every widget is placed explicitly: this component computes
    the pack and hands each frame `--dash-gc` / `--dash-gr` custom
    properties consumed by `xl:[grid-column:...]` / `xl:[grid-row:...]`.
    Below xl the grid is a single column in list order, exactly as
    before (view mode at content height with a max-height cap, edit
    mode on the fixed lattice).

Drag UX, "stable DOM, projected transforms":

  * The DOM order matches `visibleEntries` for the lifetime of the
    drag and every widget stays at its committed cell. The engine
    resolves the cursor to a pin (cell minus grab offset) and emits
    `dragState.placement`; diffing the committed pack against the
    placement's candidate pack yields a `Map<originalIndex,
    translate>` that slides every widget to its projected
    post-commit cell in 180ms. The preview is always a genuine pack
    output, so what you see mid-drag is exactly what commits on drop
    and what a reload re-derives.

  * The source widget renders with the shell's dragging styling
    (dashed accent outline + 40% body opacity) at its projected
    cell: it IS the drop preview. A title-chip ghost follows the
    cursor so the hand never feels empty, and a faint lattice
    underlay appears so the snapping is legible.

  * Commit runs once on pointerup: the packed result is re-sorted
    into reading order and written with every widget's anchor column
    as ONE undo step (`store.commitPlacement`). Because the
    pre-commit visual layout (DOM in original order + projected
    transforms) is pixel-identical to the post-commit layout, there
    is no jump. Gestures on the 1-column lattice commit order-only
    so mobile reorders never clobber desktop anchors.

  * A drop that changes nothing fires a soft outline pulse.

Resize UX:

  * Three handles per widget (WidgetFrame): right edge (width),
    bottom edge (height), SE corner (both). The gesture drives a
    live re-pack via `resizePreview`; a centered "cols × rows" badge
    reads out the snapped size; the whole gesture commits as one
    undo step. Width is clamped so the widget's right edge stops at
    the lattice edge instead of jumping columns. The context menu's
    Width / Height options preview through the same mechanism and
    revert if the menu closes without a selection.

Keyboard (shell forwards via the widget context): arrows nudge the
focused widget across the lattice (left/right change its column,
up/down move it past the vertical neighbor in its band), digits size
it. Outcomes are announced through the aria-live region.
-->
<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, toRef, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'
import {
  footprint,
  layoutsEqual,
  packAnchored,
  packWithPlacement,
  placementForPin,
  snapshotGridColumnCount,
  usePointerSortable,
  type DropPlacement,
  type GridCell,
  type LatticeSnapshot,
  type ProjectableEntry,
} from '@/composables/usePointerSortable'
import {
  effectiveColFor,
  effectiveSpanFor,
  LATTICE_COLS,
  minRowSpanFor,
  minSpanFor,
  rowSpanClass,
  rowSpanFor,
  widgetById,
  type WidgetCol,
  type WidgetDef,
  type WidgetSpan,
} from './widgets'
import type { MoveDirection, ResizeAxis, ResizePreviewIntent } from './widgetContext'
import WidgetFrame from './WidgetFrame.vue'

type LayoutEntry = {
  id: string
  span?: WidgetSpan
  rowSpan?: WidgetSpan
  col?: WidgetCol
}

const store = useDashboardLayoutStore()
const fluent = useFluent()

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

// Lattice metrics captured at drag-start, in document coordinates so
// they stay exact while the page auto-scrolls under the drag. Because
// the grid is a fixed-unit lattice (`grid-auto-rows:
// var(--dash-row-unit)`), a cell delta maps to a constant pixel
// offset, so the projected transforms can't drift from the real
// post-drop layout.
const layoutSnapshot = ref<LatticeSnapshot | null>(null)

function captureLayoutSnapshot(): LatticeSnapshot | null {
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
  return {
    originX: gridRect.left + window.scrollX,
    originY: gridRect.top + window.scrollY,
    colWidth,
    colGap,
    rowUnit,
    rowGap,
    cols,
  }
}

// Resolve each visible widget's registry def ONCE here (it feeds five
// template bindings plus the min-span lookups), so the per-render
// v-for doesn't rescan the registry ~8 times per widget.
const visibleEntries = computed(() => {
  const out: Array<{ entry: LayoutEntry; originalIndex: number; def: WidgetDef }> = []
  store.layout.widgets.forEach((entry, originalIndex) => {
    if (!entry.visible) return
    const def = widgetById(entry.id)
    if (def) out.push({ entry, originalIndex, def })
  })
  return out
})

/** Packable shape of the visible entries. Resize-preview aware: the
 *  widget mid-gesture (or under a menu hover) carries its previewed
 *  spans, so the pack below reflows live. A drag never coexists with
 *  a resize preview (mutual exclusion below), so the drag engine and
 *  its transform projection always see committed spans + anchors. */
const anchoredEntries = computed<ProjectableEntry[]>(() =>
  visibleEntries.value.map(({ entry, originalIndex }) => {
    const colSpan = colSpanOf(entry)
    return {
      originalIndex,
      colSpan,
      rowSpan: rowSpanOf(entry),
      col: effectiveColFor(entry, colSpan),
    }
  }),
)

/** The xl lattice pack: every visible widget's derived cell. Drives
 *  the explicit `grid-column` / `grid-row` placement, the keyboard
 *  adjacency scans, and the transform-diff base. */
const placements = computed<Map<number, GridCell>>(() =>
  packAnchored(anchoredEntries.value, LATTICE_COLS),
)

const { dragState, handlePointerDown, isDragged } = usePointerSortable({
  enabled: toRef(store, 'editMode'),
  onDrop: (sourceIndex, placement, pack) => commitDrop(sourceIndex, placement, pack),
  onInvalidDrop: flashSourcePulse,
  getLattice: () => {
    const snap = captureLayoutSnapshot()
    layoutSnapshot.value = snap
    return snap
  },
  getEntries: () => anchoredEntries.value,
})

/** Drag-handle pointerdown, with mutual exclusion against an
 *  in-flight resize gesture (multi-touch can attempt both). Also
 *  seeds the ghost position so the chip appears under the pointer
 *  even before the first post-drag pointermove. */
function onWidgetHandlePointerDown(originalIndex: number, e: PointerEvent) {
  if (resizeStart) return
  ghostPos = { x: e.clientX, y: e.clientY }
  handlePointerDown(originalIndex, e)
}

/** Sort a pack into reading order (row, then column), commit it as
 *  one undo step (the store drops anchors when `cols === 1`), and
 *  announce where the source landed. Returns false when the pack
 *  doesn't cover every visible widget (a no-op guard). */
function commitAndAnnounce(
  pack: Map<number, GridCell>,
  cols: number,
  sourceIndex: number,
): boolean {
  const entries = anchoredEntries.value
  const ordered = entries
    .map((e) => ({ e, cell: pack.get(e.originalIndex) }))
    .filter((x): x is { e: ProjectableEntry; cell: GridCell } => !!x.cell)
  if (ordered.length !== entries.length) return false
  ordered.sort((a, b) => a.cell.row - b.cell.row || a.cell.col - b.cell.col)
  store.commitPlacement(
    ordered.map(({ e, cell }) => ({ index: e.originalIndex, col: cell.col as WidgetCol })),
    cols,
  )
  const landed = ordered.findIndex((x) => x.e.originalIndex === sourceIndex)
  if (landed !== -1) announceMove(ordered[landed].cell, landed, ordered.length, cols)
  return true
}

function commitDrop(sourceIndex: number, placement: DropPlacement, pack: Map<number, GridCell>) {
  // The engine already packed the committed layout for its no-op
  // check and hands it over, so no re-pack here.
  commitAndAnnounce(pack, placement.cols, sourceIndex)
}

/** The drag intent's candidate pack, on the lattice width the
 *  gesture runs on. Null while idle. */
const dragPreviewPack = computed<Map<number, GridCell> | null>(() => {
  if (!dragState.isDragging || !dragState.placement) return null
  return packWithPlacement(
    anchoredEntries.value,
    dragState.sourceIndex,
    Math.max(1, dragState.placement.cols),
    dragState.placement.col,
    dragState.placement.insertion,
  )
})

const transformMap = computed<Map<number, string>>(() => {
  const map = new Map<number, string>()
  if (!dragState.isDragging) return map
  const snap = layoutSnapshot.value
  const preview = dragPreviewPack.value
  if (!snap || !preview) return map
  const cols = Math.max(1, dragState.renderedColumns)
  // The committed base is invariant for the whole drag (entries are
  // snapshotted, the store isn't written mid-drag). At xl it is the
  // `placements` computed verbatim; only the rare below-xl edit drag
  // needs a fresh pack.
  const base = cols === LATTICE_COLS ? placements.value : packAnchored(anchoredEntries.value, cols)
  if (layoutsEqual(base, preview)) return map

  // Uniform lattice: a one-cell step is a constant pixel distance on
  // each axis (track size + gap), so the transform exactly matches
  // where the widget lands after the real reflow on drop.
  const colPitch = snap.colWidth + snap.colGap
  const rowPitch = snap.rowUnit + snap.rowGap

  for (const e of anchoredEntries.value) {
    const orig = base.get(e.originalIndex)
    const proj = preview.get(e.originalIndex)
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

// Per-widget inline style: the xl placement custom properties (from the
// gravity pack), the drag transform (when dragging), and, in view mode, the
// mobile max-height cap (rowSpan × row unit) that `max-h-[var(--dash-max-h)]`
// on the frame reads (it's `xl:max-h-none` so desktop keeps the fixed span).
// During a drag the placement stays at the COMMITTED cell and the transform
// carries the projection, so pointer capture and the DOM never move.
function widgetStyle(entry: LayoutEntry, originalIndex: number) {
  const styles: Array<Record<string, string> | undefined> = [styleFor(originalIndex)]
  const cell = placements.value.get(originalIndex)
  if (cell) {
    styles.push({
      '--dash-gc': `${cell.col + 1} / span ${colSpanOf(entry)}`,
      '--dash-gr': `${cell.row + 1} / span ${rowSpanOf(entry)}`,
    })
  }
  if (!store.editMode) {
    styles.push({ '--dash-max-h': `${rowSpanOf(entry) * DASH_ROW_UNIT_REM}rem` })
  }
  return styles
}

// -- Cursor drag ghost ---------------------------------------------------
//
// A lightweight title chip that follows the pointer while a drag is
// live. The in-grid source widget is the real drop preview (dashed
// outline at its projected slot); the ghost only restores the "I'm
// holding something" feel. It is a facsimile rather than a DOM clone
// because canvas-backed charts clone blank and a full clone would be
// a large subtree. Positioning is non-reactive: a document
// pointermove listener + rAF write the transform straight to the
// element, so nothing re-renders per frame.
const ghostEl = ref<HTMLElement | null>(null)
const ghostTitle = ref('')
const ghostVisible = computed(() => dragState.isDragging && dragState.hasMoved)
let ghostPos = { x: 0, y: 0 }
let ghostRaf = 0

function scheduleGhostFrame() {
  if (ghostRaf) return
  ghostRaf = requestAnimationFrame(() => {
    ghostRaf = 0
    const el = ghostEl.value
    if (!el) return
    el.style.transform = `translate3d(${ghostPos.x + 12}px, ${ghostPos.y + 14}px, 0) scale(0.97)`
  })
}

function onGhostPointerMove(e: PointerEvent) {
  ghostPos = { x: e.clientX, y: e.clientY }
  scheduleGhostFrame()
}

watch(
  () => dragState.isDragging,
  (active) => {
    if (active) {
      const entry = store.layout.widgets[dragState.sourceIndex]
      const def = entry ? widgetById(entry.id) : undefined
      ghostTitle.value = def ? fluent.$t(def.titleKey) : ''
      document.addEventListener('pointermove', onGhostPointerMove)
    } else {
      layoutSnapshot.value = null
      document.removeEventListener('pointermove', onGhostPointerMove)
      if (ghostRaf) {
        cancelAnimationFrame(ghostRaf)
        ghostRaf = 0
      }
    }
  },
)

// Position the chip the moment it mounts (its first rAF may otherwise
// have run before the v-if inserted the element).
watch(ghostVisible, (visible) => {
  if (visible) void nextTick(() => scheduleGhostFrame())
})

// -- Resize (edge + corner handles) ---------------------------------------
//
// The handles on each widget (WidgetFrame) drive a live re-pack: as
// the pointer drags, the widget's column / row span follow it
// (snapped to lattice cells, constrained to the handle's axis,
// clamped to the registry minimums and to the lattice edge) and the
// grid reflows reactively via `anchoredEntries` -> `placements`. The
// change is committed to the store once on pointerup, so the whole
// gesture is a single undo step rather than one per frame.
const resizePreview = ref<{ id: string; span: WidgetSpan; rowSpan: WidgetSpan } | null>(null)
/** Height floor held while a menu-hover resize preview is active. A
 *  preview re-packs the grid live; without this, previewing a *smaller*
 *  size shortens the page, and if the widget sits near the bottom the
 *  scroll container clamps upward — yanking the open menu out from under
 *  the cursor. Freezing the grid's min-height at its pre-preview value
 *  lets a preview grow the page (top stays put) but never shrink it. */
const reservedMinHeight = ref<number | null>(null)
/** True only while a pointer resize gesture is in flight (not for
 *  menu hover previews). Drives the size badge + lattice underlay. */
const resizeActive = ref(false)
let resizeStart: {
  id: string
  left: number
  top: number
  colPitch: number
  rowPitch: number
  cols: number
  axis: ResizeAxis
  startSpan: WidgetSpan
  startRowSpan: WidgetSpan
  minSpan: WidgetSpan
  minRowSpan: WidgetSpan
  /** The widget's packed column at gesture start. Width clamps to
   *  `cols - packedCol` so dragging the right edge stops at the
   *  lattice edge instead of shifting the widget left. */
  packedCol: number
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

function onResizePointerDown(originalIndex: number, e: PointerEvent, axis: ResizeAxis) {
  if (!store.editMode) return
  if (e.button !== undefined && e.button !== 0) return
  // Mutual exclusion: a drag gesture owns the lattice metrics and
  // its projection reads committed spans; never resize under it.
  if (dragState.isDragging) return
  e.preventDefault()
  e.stopPropagation()
  const entry = visibleEntries.value.find((v) => v.originalIndex === originalIndex)?.entry
  const snap = captureLayoutSnapshot()
  const el = gridEl.value?.querySelector<HTMLElement>(`[data-sortable-index="${originalIndex}"]`)
  if (!entry || !snap || !el) return

  const rect = el.getBoundingClientRect()
  const startSpan = effectiveSpanFor(entry)
  const startRowSpan = rowSpanFor(entry)
  resizeStart = {
    id: entry.id,
    left: rect.left,
    top: rect.top,
    colPitch: snap.colWidth + snap.colGap,
    rowPitch: snap.rowUnit + snap.rowGap,
    cols: snap.cols,
    axis,
    startSpan,
    startRowSpan,
    minSpan: minSpanFor(entry.id),
    minRowSpan: minRowSpanFor(entry.id),
    packedCol: placements.value.get(originalIndex)?.col ?? 0,
  }
  resizePreview.value = { id: entry.id, span: startSpan, rowSpan: startRowSpan }
  resizeActive.value = true
  resizePointerId = e.pointerId
  try {
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
  } catch {
    // capture is best-effort; the document listeners still track the drag.
  }
  document.addEventListener('pointermove', onResizeMove)
  document.addEventListener('pointerup', onResizeUp)
  document.addEventListener('pointercancel', onResizeCancel)
  document.addEventListener('lostpointercapture', onResizeLostCapture)
}

function onResizeMove(e: PointerEvent) {
  if (!resizeStart || e.pointerId !== resizePointerId) return
  // Span counted from the widget's gesture-start top-left, snapped to
  // the nearest cell, constrained to the handle's axis, and clamped:
  // columns to what fits right of the widget's anchor, rows to the
  // 1-3 lattice range, both to the widget's registry minimum.
  const s = resizeStart
  let span = s.startSpan
  let rowSpan = s.startRowSpan
  if (s.axis !== 'y') {
    const raw = Math.round((e.clientX - s.left) / s.colPitch)
    span = Math.max(s.minSpan, Math.min(s.cols - s.packedCol, raw)) as WidgetSpan
  }
  if (s.axis !== 'x') {
    const raw = Math.round((e.clientY - s.top) / s.rowPitch)
    rowSpan = Math.max(s.minRowSpan, Math.min(3, raw)) as WidgetSpan
  }
  // Skip the write when the snapped size hasn't changed; otherwise a
  // stationary drag re-packs and re-renders the grid every frame.
  const prev = resizePreview.value
  if (prev && prev.span === span && prev.rowSpan === rowSpan) return
  resizePreview.value = { id: s.id, span, rowSpan }
}

function endResize() {
  document.removeEventListener('pointermove', onResizeMove)
  document.removeEventListener('pointerup', onResizeUp)
  document.removeEventListener('pointercancel', onResizeCancel)
  document.removeEventListener('lostpointercapture', onResizeLostCapture)
  resizeStart = null
  resizeActive.value = false
  resizePointerId = -1
}

function onResizeUp(e: PointerEvent) {
  if (e.pointerId !== resizePointerId) return
  const preview = resizePreview.value
  const start = resizeStart
  endResize()
  resizePreview.value = null
  if (preview && start) {
    // Atomic commit: both axes land as one undo step.
    store.setSpans(start.id, { span: preview.span, rowSpan: preview.rowSpan })
    announceSize(start.id)
  }
}

function onResizeCancel() {
  endResize()
  resizePreview.value = null
}

/** iOS Safari can steal pointer capture (long-press callout); treat
 *  it as a gesture cancel so the preview never strands. */
function onResizeLostCapture(e: PointerEvent) {
  if (!resizeStart || e.pointerId !== resizePointerId) return
  onResizeCancel()
}

onBeforeUnmount(endResize)

/** "cols × rows" readout for the widget a resize gesture is
 *  reshaping; null for everything else (including menu previews). */
function badgeFor(entry: LayoutEntry): string | null {
  if (!resizeActive.value) return null
  const p = resizePreview.value
  if (!p || p.id !== entry.id) return null
  return fluent.$t('dashboard-widget-resize-badge', { cols: p.span, rows: p.rowSpan })
}

// -- Menu hover preview ----------------------------------------------------

/** Context-menu Width/Height hover preview. Writes `resizePreview`
 *  only (never the store), so the grid reflows live and closing the
 *  menu without selecting reverts by clearing the ref. Ignored while
 *  a drag or resize gesture owns the layout. */
function onPreviewResize(entry: LayoutEntry, intent: ResizePreviewIntent | null) {
  if (dragState.isDragging || resizeStart) return
  if (intent === null) {
    resizePreview.value = null
    return
  }
  resizePreview.value = {
    id: entry.id,
    span: intent.span ?? effectiveSpanFor(entry),
    rowSpan: intent.rowSpan ?? rowSpanFor(entry),
  }
}

/** The resize context menu opened or closed. Hold the grid's height at
 *  its open-time value for the whole session so any preview (including a
 *  smaller one) can't shorten the page and jump the scroll out from
 *  under the open menu. Released on close, which also reverts any
 *  uncommitted preview. */
function onSizeMenuToggle(open: boolean) {
  if (open) {
    reservedMinHeight.value = gridEl.value?.offsetHeight ?? null
  } else {
    reservedMinHeight.value = null
    resizePreview.value = null
  }
}

// -- Committed resizes (menu select, keyboard digits) ----------------------

function commitSpan(entry: LayoutEntry, span: WidgetSpan) {
  store.setSpans(entry.id, { span })
  announceSize(entry.id)
}

function commitRowSpan(entry: LayoutEntry, rowSpan: WidgetSpan) {
  store.setSpans(entry.id, { rowSpan })
  announceSize(entry.id)
}

function announceSize(id: string) {
  // Read back from the store so clamping is reflected in the readout.
  const entry = store.layout.widgets.find((w) => w.id === id)
  if (!entry) return
  announce(
    fluent.$t('dashboard-widget-a11y-resized', {
      cols: effectiveSpanFor(entry),
      rows: rowSpanFor(entry),
    }),
  )
}

// -- Keyboard move ----------------------------------------------------------

/** Announce a completed move. `position` is the source's index in
 *  the committed reading order (derived from the pre-commit pack;
 *  the commit renumbers array indices, so no store lookup here). */
function announceMove(cell: GridCell, position: number, total: number, cols: number) {
  if (cols === 1) {
    // Single-column lattice: a cell readout is meaningless, announce
    // the list position instead.
    announce(
      fluent.$t('dashboard-widget-a11y-moved', {
        position: position + 1,
        total,
      }),
    )
    return
  }
  announce(
    fluent.$t('dashboard-widget-a11y-moved-cell', {
      col: cell.col + 1,
      cols,
      row: cell.row + 1,
    }),
  )
}

/** Keyboard repositioning on the packed lattice. Left/right nudge
 *  the anchor column; up/down pin the widget at the row of its
 *  vertical neighbor in the column band (pushing it aside), with an
 *  order step as fallback. All paths commit through the same pack ->
 *  reading-order -> `commitPlacement` pipeline as a drag, one undo
 *  step each. On the single-column lattice, moves are order steps
 *  and commit order-only. */
function moveWidget(originalIndex: number, dir: MoveDirection) {
  const entries = anchoredEntries.value
  const vIdx = entries.findIndex((e) => e.originalIndex === originalIndex)
  if (vIdx === -1) return
  const cols = Math.max(1, snapshotGridColumnCount(gridEl.value))

  if (cols === 1) {
    const insertion = dir === 'left' || dir === 'up' ? vIdx - 1 : vIdx + 1
    const pack = packWithPlacement(entries, originalIndex, 1, 0, insertion)
    if (!pack || !commitAndAnnounce(pack, 1, originalIndex)) flashSourcePulse(originalIndex)
    return
  }

  const base = placements.value
  const src = entries[vIdx]
  const cell = base.get(originalIndex)
  if (!cell) return
  const { w, h } = footprint(src, cols)

  let pin: GridCell | null = null
  if (dir === 'left' || dir === 'right') {
    const nextCol = cell.col + (dir === 'left' ? -1 : 1)
    if (nextCol < 0 || nextCol > cols - w) {
      flashSourcePulse(originalIndex)
      return
    }
    pin = { row: cell.row, col: nextCol }
  } else {
    // The nearest widget whose footprint overlaps the source's
    // column band, above (for up) or below (for down).
    let neighborRow: number | null = null
    for (const e of entries) {
      if (e.originalIndex === originalIndex) continue
      const c = base.get(e.originalIndex)
      if (!c) continue
      const ew = footprint(e, cols).w
      if (c.col >= cell.col + w || cell.col >= c.col + ew) continue
      if (dir === 'up') {
        const top = c.row
        if (top < cell.row && (neighborRow === null || top > neighborRow)) neighborRow = top
      } else {
        const top = c.row
        if (
          top > cell.row + h - 1 &&
          (neighborRow === null || top < neighborRow)
        ) {
          neighborRow = top
        }
      }
    }
    if (neighborRow !== null) {
      pin = { row: neighborRow, col: cell.col }
    } else if (dir === 'up' && cell.row > 0) {
      pin = { row: cell.row - 1, col: cell.col }
    } else if (dir === 'down') {
      pin = { row: cell.row + h, col: cell.col }
    }
    if (!pin) {
      flashSourcePulse(originalIndex)
      return
    }
  }

  const insertion = placementForPin(entries, originalIndex, cols, pin, vIdx)
  if (insertion === null) {
    flashSourcePulse(originalIndex)
    return
  }
  const chosen = packWithPlacement(entries, originalIndex, cols, pin.col, insertion)
  if (!chosen || layoutsEqual(chosen, base)) {
    flashSourcePulse(originalIndex)
    return
  }
  commitAndAnnounce(chosen, cols, originalIndex)
}

// -- Screen-reader announcements -------------------------------------------

const liveMessage = ref('')

/** Clear-then-set so repeating an identical message re-announces. */
function announce(msg: string) {
  liveMessage.value = ''
  void nextTick(() => {
    liveMessage.value = msg
  })
}

// -- Lattice underlay --------------------------------------------------------

/** True while any pointer gesture is reshaping the layout. Shows the
 *  lattice underlay so the cell snapping is legible. */
const gestureActive = computed(() => dragState.isDragging || resizeActive.value)

/** Cell count for the underlay: the packed content extent (of the
 *  drag preview while dragging, so gravity previews that extend a
 *  row are covered) plus one spare row as the drop-below / grow-into
 *  affordance. Recomputes on snap changes only, never per pointer
 *  frame. */
const underlayCellCount = computed(() => {
  if (!gestureActive.value) return 0
  const dragging = dragState.isDragging
  // Column count is already snapshotted per gesture (renderedColumns
  // for a drag, resizeStart.cols for a resize); avoid a getComputedStyle
  // read per recompute.
  const cols = Math.max(1, dragging ? dragState.renderedColumns : (resizeStart?.cols ?? LATTICE_COLS))
  const entries = anchoredEntries.value
  const pack = dragging
    ? (dragPreviewPack.value ?? packAnchored(entries, cols))
    : cols === LATTICE_COLS
      ? placements.value
      : packAnchored(entries, cols)
  let maxBottom = 0
  for (const e of entries) {
    const c = pack.get(e.originalIndex)
    if (c) maxBottom = Math.max(maxBottom, c.row + Math.max(1, e.rowSpan))
  }
  return cols * (maxBottom + 1)
})
</script>

<template>
  <div
    ref="gridEl"
    :class="[
      'relative grid grid-cols-1 xl:grid-cols-3 transition-[gap] duration-150',
      gridAutoRows,
      gridGap,
      store.editMode && 'select-none',
      dragState.isDragging && 'cursor-grabbing',
    ]"
    :style="{
      '--dash-row-unit': '8.5rem',
      minHeight: reservedMinHeight !== null ? `${reservedMinHeight}px` : undefined,
    }"
  >
    <!-- Lattice underlay: faint cell outlines while a drag or resize
         gesture is live, so the snapping is legible. Absolutely
         positioned (not a grid item) and mirrors the real grid
         template, so its cells align with the lattice by
         construction. Sits under the widgets (z-0 vs their stacking
         contexts) and never intercepts the pointer. -->
    <Transition name="dash-underlay">
      <div
        v-if="gestureActive"
        aria-hidden="true"
        class="absolute inset-0 z-0 pointer-events-none grid grid-cols-1 xl:grid-cols-3 [grid-auto-rows:var(--dash-row-unit)] gap-4"
      >
        <div
          v-for="i in underlayCellCount"
          :key="i"
          class="rounded-xl border border-default/60 bg-surface-alt/30"
        />
      </div>
    </Transition>

    <WidgetFrame
      v-for="{ entry, originalIndex, def } in visibleEntries"
      :key="entry.id"
      :index="originalIndex"
      :current-span="colSpanOf(entry)"
      :current-row-span="rowSpanOf(entry)"
      :min-span="def.minSpan ?? 1"
      :min-row-span="def.minRowSpan ?? 1"
      :edit-mode="store.editMode"
      :dragging="isDragged(originalIndex)"
      :pulsing="pulseSourceIndex === originalIndex"
      :size-badge="badgeFor(entry)"
      :component="def.component"
      :widget-props="def.props"
      :frame-wraps="def.frameWraps ?? false"
      :frame-title-key="def.titleKey"
      :body-aspect="def.bodyAspect"
      :class="[
        'xl:[grid-column:var(--dash-gc)] xl:[grid-row:var(--dash-gr)]',
        rowSpanClass(rowSpanOf(entry), store.editMode),
        !store.editMode && 'max-h-[var(--dash-max-h)] xl:max-h-none',
        'widget-projected',
      ]"
      :style="widgetStyle(entry, originalIndex)"
      @hide="store.hide(entry.id)"
      @resize="(span) => commitSpan(entry, span)"
      @resize-row="(rowSpan) => commitRowSpan(entry, rowSpan)"
      @preview-resize="(intent) => onPreviewResize(entry, intent)"
      @size-menu-toggle="(open) => onSizeMenuToggle(open)"
      @move="(dir) => moveWidget(originalIndex, dir)"
      @handle-pointerdown="(e) => onWidgetHandlePointerDown(originalIndex, e)"
      @resize-pointerdown="(e, axis) => onResizePointerDown(originalIndex, e, axis)"
    />

    <!-- Cursor-following drag ghost. Fixed-position (not a grid
         item); its transform is written directly by the rAF loop, so
         it renders once per drag and costs nothing per frame. Parked
         off-screen until the first frame lands. -->
    <div
      v-if="ghostVisible"
      ref="ghostEl"
      aria-hidden="true"
      class="fixed left-0 top-0 z-[100] pointer-events-none select-none flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-surface border border-default shadow-lg text-xs font-medium text-primary opacity-90 will-change-transform"
      style="transform: translate3d(-9999px, -9999px, 0)"
    >
      <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true" class="text-tertiary">
        <path
          d="M3 2h.01M7 2h.01M3 5h.01M7 5h.01M3 8h.01M7 8h.01"
          stroke="currentColor"
          stroke-width="1.6"
          stroke-linecap="round"
        />
      </svg>
      <span>{{ ghostTitle }}</span>
    </div>

    <!-- Move / resize outcomes for screen readers. -->
    <div class="sr-only" role="status" aria-live="polite">{{ liveMessage }}</div>
  </div>
</template>

<style scoped>
/* Every widget animates its own transform. During a drag the
 * transform map updates on every accepted cell change; without this
 * transition the projected layout would snap instead of slide. */
.widget-projected {
  transition: transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

/* Lattice underlay fade. Opacity only, so it stays subtle; removed
 * entirely under prefers-reduced-motion (it appears/disappears
 * instantly, which is the reduced-motion contract). */
.dash-underlay-enter-active,
.dash-underlay-leave-active {
  transition: opacity 150ms ease;
}
.dash-underlay-enter-from,
.dash-underlay-leave-to {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .widget-projected {
    transition: none;
  }
  .dash-underlay-enter-active,
  .dash-underlay-leave-active {
    transition: none;
  }
}
</style>
