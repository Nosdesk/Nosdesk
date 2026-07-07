/**
 * Pointer-event sortable grid on a fixed-unit lattice. The engine is
 * a pure state machine emitting drop intent; the view decides how to
 * render it.
 *
 * HTML5 DnD is avoided across the app (touch support, dragleave-on-
 * children, crippled drag image). This composable uses Pointer
 * Events, same pattern as `useKanbanDragDrop`.
 *
 * Layout model: the dashboard persists list order (vertical intent)
 * plus an optional anchor column per widget. Rows are always DERIVED
 * by the gravity packer `packAnchored`: widgets float up within their
 * column band, collisions push down. This is the react-grid-layout /
 * gridstack compaction model. Placement is therefore free within a
 * column (the dragged widget's column always honors the cursor) while
 * layouts stay tidy without persisting rows.
 *
 * Rendering contract:
 *   - The view renders items with `data-sortable-index="${n}"` on
 *     each item's root element. `n` is the ORIGINAL index into the
 *     underlying array.
 *   - The view wires `@pointerdown` on a drag handle inside each
 *     item to `handlePointerDown(index, event)`.
 *   - The view DOES NOT reorder its rendered DOM during the drag.
 *     It derives a per-widget `transform: translate(dx, dy)` map by
 *     diffing the committed pack against the drag candidate pack
 *     (`packWithPlacement` of `dragState.placement`). CSS transitions
 *     on `transform` animate the slide. Keeping the DOM stable means
 *     pointer capture survives the drag and no FLIP machinery has to
 *     fight per-cursor-move re-renders. The transforms are exact
 *     because the grid is a fixed-unit lattice (`grid-auto-rows`), so
 *     a lattice-cell delta maps to a constant pixel offset.
 *   - On drop the engine calls `onDrop(sourceIndex, placement, pack)`;
 *     the view owns the commit (reading-order re-sort + anchor writes).
 *
 * Drag targeting:
 *   - The view supplies `getLattice` (grid origin in DOCUMENT
 *     coordinates plus track/gap pitches) and `getEntries` (the
 *     visible entries in list order), both snapshotted at drag
 *     start. Document coordinates make the snapshot exact through
 *     any amount of scrolling.
 *   - Every pointer position resolves to a lattice cell via
 *     `cellFromPoint` (with hysteresis), then to a PIN: the cell
 *     minus the grab offset captured at drag start, so large widgets
 *     track under the hand instead of snapping their top-left to the
 *     cursor.
 *   - The pin converts to intent with `placementForPin`: the source's
 *     anchor column becomes the pin column, and the insertion slot is
 *     the one whose gravity pack lands the source's footprint nearest
 *     the pin row. The preview is always a genuine pack output (never
 *     the raw pin cell), so what you see mid-drag is exactly what
 *     commits on drop and what re-derives on reload.
 *   - Stability rules pin the preview so pack-equivalent intents
 *     can't make it flutter: the current insertion wins whenever it
 *     is still score-optimal, or when the best candidate packs to an
 *     identical layout.
 *
 * Touch + iOS Safari hardening:
 *   - `lostpointercapture` is treated as cancel (iOS Safari long-
 *     press contextmenu can steal capture).
 *   - `contextmenu` is `preventDefault`'d while dragging so the iOS
 *     long-press menu doesn't appear under the finger.
 *   - Auto-scroll: when the cursor is within `EDGE_SCROLL_BAND` of
 *     the viewport top or bottom, a rAF loop scrolls the document at
 *     a rate proportional to edge proximity. Each scroll tick re-runs
 *     targeting from the last pointer position, because no pointermove
 *     fires while the pointer is stationary during programmatic
 *     scroll.
 */
import { onBeforeUnmount, onMounted, reactive, type Ref } from 'vue'

/** The engine's drop intent: insert the source at `insertion` (index
 *  into the source-removed visible list) with its anchor column set
 *  to `col`. `cols` is the lattice width the gesture ran on, carried
 *  through because the engine resets its state before invoking
 *  `onDrop` (the view needs it to make 1-column commits order-only). */
export interface DropPlacement {
  insertion: number
  col: number
  cols: number
}

export interface PointerSortableOptions {
  enabled: Ref<boolean>
  /** Commit handler. Receives the intent plus the already-computed
   *  gravity pack, so the view never re-packs to commit. */
  onDrop: (
    sourceIndex: number,
    placement: DropPlacement,
    pack: Map<number, GridCell>,
  ) => void
  /** Called on pointerup with no movement or a no-op target so the
   *  caller can run the soft outline pulse on the source widget.
   *  Optional; default is a no-op. */
  onInvalidDrop?: (sourceIndex: number) => void
  /** Lattice metrics snapshot provider, called once at drag start.
   *  Returning null aborts the drag (nothing to target against). */
  getLattice: () => LatticeSnapshot | null
  /** Visible entries in list order, INCLUDING the drag source.
   *  Called once at drag start alongside `getLattice`. */
  getEntries: () => ProjectableEntry[]
  clickThreshold?: number
  touchHoldMs?: number
}

/** Grid geometry captured at drag start, in DOCUMENT coordinates so
 *  it stays exact while the page auto-scrolls under the drag. */
export interface LatticeSnapshot {
  /** Grid content origin: boundingClientRect + window scroll. */
  originX: number
  originY: number
  colWidth: number
  colGap: number
  rowUnit: number
  rowGap: number
  cols: number
}

export interface DragState {
  isDragging: boolean
  /** Original index of the dragged item, or -1 when idle. */
  sourceIndex: number
  /** Snapshot of the grid container's rendered column count at
   *  drag-start. At xl+ this is typically 3; below xl the grid
   *  collapses to 1 and the math degrades cleanly to a single-
   *  column list. */
  renderedColumns: number
  /** The current drop intent. Null when idle. Starts as the source's
   *  own slot (a no-op). */
  placement: DropPlacement | null
  /** True once the pointer exceeded the click threshold. Gates
   *  cursor-following chrome (the drag ghost) so a bare click on a
   *  drag handle doesn't flash it. */
  hasMoved: boolean
}

/** Cursor must be within this many pixels of the viewport top or
 *  bottom for the auto-scroll rAF loop to fire. */
const EDGE_SCROLL_BAND = 60
/** Maximum scroll velocity (px per frame) at the very edge. Falls
 *  off linearly to 0 at the band boundary. */
const EDGE_SCROLL_MAX = 4
/** A new lattice cell is accepted only once the cursor is this many
 *  pixels beyond the accepted cell's tile, so the projected preview
 *  doesn't flap when the cursor rides a cell boundary. */
const CELL_HYSTERESIS_PX = 8

/** Count rendered grid columns from `grid-template-columns`. Falls
 *  back to 1 when the grid is single-column (mobile) or unavailable. */
export function snapshotGridColumnCount(el: HTMLElement | null): number {
  if (!el) return 1
  const tracks = getComputedStyle(el).gridTemplateColumns.trim()
  if (!tracks || tracks === 'none') return 1
  return tracks.split(/\s+/).filter(Boolean).length
}

export function usePointerSortable(options: PointerSortableOptions) {
  const { enabled, onDrop, onInvalidDrop, getLattice, getEntries } = options
  const clickThreshold = options.clickThreshold ?? 5
  const touchHoldMs = options.touchHoldMs ?? 400

  const dragState = reactive<DragState>({
    isDragging: false,
    sourceIndex: -1,
    renderedColumns: 1,
    placement: null,
    hasMoved: false,
  })

  // Snapshotted at drag start.
  let lattice: LatticeSnapshot | null = null
  let latticeEntries: ProjectableEntry[] = []
  /** Committed layout at drag start; drop compares against it so a
   *  pack-identical drop registers as a no-op pulse. */
  let baselinePack: Map<number, GridCell> | null = null
  /** Grab offset: where inside the source's footprint the drag
   *  started, in cells. Subtracted from the cursor cell so large
   *  widgets track under the hand. */
  let grabRowOff = 0
  let grabColOff = 0
  /** Source footprint (w, h), clamped to the lattice. */
  let srcW = 1
  let acceptedCell: GridCell | null = null

  let startPos = { x: 0, y: 0 }
  let lastPointer = { x: 0, y: 0 }
  let pendingIndex = -1
  let pendingPointerId = -1
  let pendingTarget: HTMLElement | null = null
  let activePointerId = -1
  let holdTimer: ReturnType<typeof setTimeout> | null = null
  let pointerMoved = false
  let autoScrollRaf = 0

  function reset() {
    if (holdTimer) {
      clearTimeout(holdTimer)
      holdTimer = null
    }
    if (autoScrollRaf) {
      cancelAnimationFrame(autoScrollRaf)
      autoScrollRaf = 0
    }
    lattice = null
    latticeEntries = []
    baselinePack = null
    grabRowOff = 0
    grabColOff = 0
    srcW = 1
    acceptedCell = null
    pendingIndex = -1
    pendingPointerId = -1
    pendingTarget = null
    activePointerId = -1
    pointerMoved = false
    dragState.isDragging = false
    dragState.sourceIndex = -1
    dragState.renderedColumns = 1
    dragState.placement = null
    dragState.hasMoved = false
  }

  /** Pixel tile of a cell in document coordinates. The tile spans a
   *  full pitch (track + trailing gap), matching `cellFromPoint`'s
   *  flooring, so tiles partition the plane with no dead zones. */
  function cellTile(snap: LatticeSnapshot, cell: GridCell) {
    const colPitch = snap.colWidth + snap.colGap
    const rowPitch = snap.rowUnit + snap.rowGap
    return {
      left: snap.originX + cell.col * colPitch,
      top: snap.originY + cell.row * rowPitch,
      right: snap.originX + (cell.col + 1) * colPitch,
      bottom: snap.originY + (cell.row + 1) * rowPitch,
    }
  }

  /** Resolve the cursor to a lattice cell and, if the cell (after
   *  hysteresis) changed, derive the pin and the new drop intent. */
  function updateTarget(clientX: number, clientY: number) {
    if (!lattice) return
    const docX = clientX + window.scrollX
    const docY = clientY + window.scrollY
    const raw = cellFromPoint(lattice, docX, docY)

    if (acceptedCell) {
      if (raw.row === acceptedCell.row && raw.col === acceptedCell.col) return
      const tile = cellTile(lattice, acceptedCell)
      if (
        docX >= tile.left - CELL_HYSTERESIS_PX &&
        docX <= tile.right + CELL_HYSTERESIS_PX &&
        docY >= tile.top - CELL_HYSTERESIS_PX &&
        docY <= tile.bottom + CELL_HYSTERESIS_PX
      ) {
        return
      }
    }

    acceptedCell = raw
    const pin: GridCell = {
      row: Math.max(0, raw.row - grabRowOff),
      col: Math.max(0, Math.min(lattice.cols - srcW, raw.col - grabColOff)),
    }

    const insertion = placementForPin(
      latticeEntries,
      dragState.sourceIndex,
      lattice.cols,
      pin,
      dragState.placement?.insertion ?? null,
    )
    if (insertion !== null) {
      dragState.placement = { insertion, col: pin.col, cols: lattice.cols }
    }
  }

  function tickAutoScroll() {
    autoScrollRaf = 0
    if (!dragState.isDragging) return
    const y = lastPointer.y
    const viewH = window.innerHeight
    let delta = 0
    if (y < EDGE_SCROLL_BAND) {
      const distance = Math.max(0, y)
      const ratio = 1 - distance / EDGE_SCROLL_BAND
      delta = -Math.round(EDGE_SCROLL_MAX * ratio)
    } else if (y > viewH - EDGE_SCROLL_BAND) {
      const distance = Math.max(0, viewH - y)
      const ratio = 1 - distance / EDGE_SCROLL_BAND
      delta = Math.round(EDGE_SCROLL_MAX * ratio)
    }
    if (delta !== 0) {
      window.scrollBy(0, delta)
      // The pointer is stationary while the document scrolls under
      // it, so no pointermove fires; re-run targeting from the last
      // known position (updateTarget reads the fresh scroll offset).
      updateTarget(lastPointer.x, lastPointer.y)
    }
    autoScrollRaf = requestAnimationFrame(tickAutoScroll)
  }

  function maybeScheduleAutoScroll() {
    if (!dragState.isDragging) return
    if (autoScrollRaf !== 0) return
    autoScrollRaf = requestAnimationFrame(tickAutoScroll)
  }

  function onPointerMove(e: PointerEvent) {
    // Touch/pen hold-to-drag: cancel if the finger moves before the
    // hold timer fires.
    if (pendingIndex !== -1 && holdTimer) {
      const dx = Math.abs(e.clientX - startPos.x)
      const dy = Math.abs(e.clientY - startPos.y)
      if (dx > clickThreshold || dy > clickThreshold) {
        clearTimeout(holdTimer)
        holdTimer = null
        pendingIndex = -1
      }
      return
    }

    if (!dragState.isDragging) return

    lastPointer = { x: e.clientX, y: e.clientY }
    const dx = Math.abs(e.clientX - startPos.x)
    const dy = Math.abs(e.clientY - startPos.y)
    if (dx > clickThreshold || dy > clickThreshold) {
      pointerMoved = true
      dragState.hasMoved = true
    }

    updateTarget(e.clientX, e.clientY)
    maybeScheduleAutoScroll()
  }

  function onPointerUp() {
    if (holdTimer) {
      clearTimeout(holdTimer)
      holdTimer = null
      pendingIndex = -1
      return
    }

    if (!dragState.isDragging) {
      reset()
      return
    }

    const { sourceIndex, placement } = dragState
    const cols = lattice?.cols ?? 1
    const entries = latticeEntries
    const baseline = baselinePack
    const moved = pointerMoved
    reset()

    if (!moved || sourceIndex === -1 || !placement || !baseline) {
      if (sourceIndex !== -1) onInvalidDrop?.(sourceIndex)
      return
    }
    const chosen = packWithPlacement(entries, sourceIndex, cols, placement.col, placement.insertion)
    if (!chosen || layoutsEqual(chosen, baseline)) {
      onInvalidDrop?.(sourceIndex)
      return
    }
    onDrop(sourceIndex, placement, chosen)
  }

  function onLostPointerCapture(e: PointerEvent) {
    if (!dragState.isDragging) return
    if (activePointerId !== -1 && e.pointerId !== activePointerId) return
    reset()
  }

  function onContextMenu(e: Event) {
    if (dragState.isDragging) e.preventDefault()
  }

  function beginDrag(index: number, pointerId: number, target: HTMLElement | null) {
    lattice = getLattice()
    latticeEntries = lattice ? getEntries() : []
    const srcVIdx = latticeEntries.findIndex((e) => e.originalIndex === index)
    const src = srcVIdx === -1 ? null : latticeEntries[srcVIdx]
    if (!lattice || !src) {
      // No lattice or the source isn't in the entry snapshot; there
      // is nothing to target against, so the drag is a no-op.
      reset()
      return
    }

    dragState.isDragging = true
    dragState.sourceIndex = index
    dragState.renderedColumns = lattice.cols

    baselinePack = packAnchored(latticeEntries, lattice.cols)
    const srcCell = baselinePack.get(index) ?? { row: 0, col: 0 }
    const { w: fw, h: fh } = footprint(src, lattice.cols)
    srcW = fw
    // Grab offset: which cell of the footprint the pointer went down
    // in, clamped into the footprint.
    const startCell = cellFromPoint(
      lattice,
      startPos.x + window.scrollX,
      startPos.y + window.scrollY,
    )
    grabRowOff = Math.max(0, Math.min(fh - 1, startCell.row - srcCell.row))
    grabColOff = Math.max(0, Math.min(fw - 1, startCell.col - srcCell.col))
    dragState.placement = { insertion: srcVIdx, col: srcCell.col, cols: lattice.cols }

    updateTarget(startPos.x, startPos.y)

    if (target && pointerId >= 0) {
      try {
        target.setPointerCapture(pointerId)
        activePointerId = pointerId
      } catch {
        // Safari occasionally rejects capture on transformed elements;
        // drag still works via the document-level listeners.
      }
    }
  }

  function handlePointerDown(index: number, e: PointerEvent) {
    if (!enabled.value) return
    if (e.button !== undefined && e.button !== 0) return

    startPos = { x: e.clientX, y: e.clientY }
    lastPointer = { x: e.clientX, y: e.clientY }
    pointerMoved = false

    if (e.pointerType === 'mouse') {
      e.preventDefault()
      beginDrag(index, e.pointerId, e.target as HTMLElement | null)
      return
    }

    pendingIndex = index
    pendingPointerId = e.pointerId
    pendingTarget = e.target as HTMLElement | null
    holdTimer = setTimeout(() => {
      if (pendingIndex === -1) return
      beginDrag(pendingIndex, pendingPointerId, pendingTarget)
      pendingIndex = -1
      pendingPointerId = -1
      pendingTarget = null
      holdTimer = null
      navigator.vibrate?.(30)
    }, touchHoldMs)
  }

  function isDragged(index: number) {
    return dragState.isDragging && dragState.sourceIndex === index
  }

  onMounted(() => {
    document.addEventListener('pointermove', onPointerMove)
    document.addEventListener('pointerup', onPointerUp)
    document.addEventListener('pointercancel', reset)
    document.addEventListener('lostpointercapture', onLostPointerCapture)
    document.addEventListener('contextmenu', onContextMenu)
  })

  onBeforeUnmount(() => {
    document.removeEventListener('pointermove', onPointerMove)
    document.removeEventListener('pointerup', onPointerUp)
    document.removeEventListener('pointercancel', reset)
    document.removeEventListener('lostpointercapture', onLostPointerCapture)
    document.removeEventListener('contextmenu', onContextMenu)
    if (holdTimer) clearTimeout(holdTimer)
    if (autoScrollRaf) cancelAnimationFrame(autoScrollRaf)
  })

  return { dragState, handlePointerDown, isDragged }
}

// ---------------------------------------------------------------------------
// Lattice geometry + packing helpers
// ---------------------------------------------------------------------------

/** Minimum shape the packing helpers need from each visible entry:
 *  a stable original index (so the caller can compare it against
 *  `dragState.sourceIndex`), the entry's column and row spans on the
 *  fixed-unit grid lattice, and its optional anchor column. */
export interface ProjectableEntry {
  originalIndex: number
  colSpan: number
  rowSpan: number
  /** Anchor column (0-based). Absent = auto: the entry packs into
   *  the earliest free slot in reading order, which is exactly the
   *  legacy CSS dense-flow behaviour. */
  col?: number
}

/** A (row, col) cell on the grid lattice, both 0-based and measured
 *  in lattice units (not pixels). */
export interface GridCell {
  row: number
  col: number
}

/** An entry's footprint, clamped to the lattice: width capped at the
 *  column count, both dimensions at least 1. */
export function footprint(entry: ProjectableEntry, cols: number): { w: number; h: number } {
  return {
    w: Math.max(1, Math.min(entry.colSpan, cols)),
    h: Math.max(1, entry.rowSpan),
  }
}

/**
 * The lattice cell under a document-space point. Positions left of /
 * above the grid clamp to the first column / row; positions right of
 * the last column clamp to it; rows are unbounded below (dropping
 * under all content pins into the trailing region). Because the pitch
 * includes the gap, a point inside a gap floors into the cell to its
 * left / top, so every point resolves to exactly one cell.
 */
export function cellFromPoint(
  snap: LatticeSnapshot,
  docX: number,
  docY: number,
): GridCell {
  const colPitch = snap.colWidth + snap.colGap
  const rowPitch = snap.rowUnit + snap.rowGap
  const col = Math.max(
    0,
    Math.min(snap.cols - 1, Math.floor((docX - snap.originX) / colPitch)),
  )
  const row = Math.max(0, Math.floor((docY - snap.originY) / rowPitch))
  return { row, col }
}

/** Occupancy lattice grown lazily one row at a time, shared by both
 *  packers. Returns fit/occupy plus the earliest row-major free slot
 *  finder (the auto-placement scan). */
function makeOccupancy(cols: number) {
  const occ: boolean[][] = []
  const ensureRow = (r: number) => {
    while (occ.length <= r) occ.push(new Array<boolean>(cols).fill(false))
  }
  const fits = (r: number, c: number, w: number, h: number): boolean => {
    if (c < 0 || c + w > cols) return false
    for (let i = r; i < r + h; i++) {
      ensureRow(i)
      for (let j = c; j < c + w; j++) if (occ[i][j]) return false
    }
    return true
  }
  const occupy = (r: number, c: number, w: number, h: number) => {
    for (let i = r; i < r + h; i++) {
      ensureRow(i)
      for (let j = c; j < c + w; j++) occ[i][j] = true
    }
  }
  /** Earliest (row-major) free slot that fits w x h. */
  const autoSlot = (w: number, h: number): GridCell => {
    for (let r = 0; ; r++) {
      for (let c = 0; c + w <= cols; c++) {
        if (fits(r, c, w, h)) return { row: r, col: c }
      }
    }
  }
  return { fits, occupy, autoSlot }
}

/**
 * Legacy dense packer, mirroring CSS `grid-auto-flow: row dense`:
 * each entry takes the earliest (row-major) free slot that fits.
 * Kept as the reference for migrating stored layouts that predate
 * anchor columns (`col`), so the one-time anchor derivation is
 * byte-identical to what those users see today. Do not change its
 * behaviour.
 */
export function packGrid(
  entries: readonly ProjectableEntry[],
  cols: number,
): Map<number, GridCell> {
  const out = new Map<number, GridCell>()
  if (cols < 1) return out
  const { occupy, autoSlot } = makeOccupancy(cols)
  for (const e of entries) {
    const { w, h } = footprint(e, cols)
    const slot = autoSlot(w, h)
    occupy(slot.row, slot.col, w, h)
    out.set(e.originalIndex, slot)
  }
  return out
}

/**
 * Gravity packer (react-grid-layout compaction semantics). Entries
 * are processed in list order:
 *
 *   - anchored (`col` set): the entry stays in its column band
 *     (clamped so the span fits) and floats up to the smallest row
 *     where its footprint is free. Widgets in the same band placed
 *     later land below, which is how collisions "push down".
 *   - auto (`col` absent): earliest row-major free slot, identical
 *     to the legacy dense flow. Used by legacy entries and widgets
 *     freshly added from the picker; the next placement commit
 *     materializes their column.
 *
 * Pure and deterministic: the renderer, the drag preview, and the
 * commit path all derive positions from this one function, which is
 * what guarantees preview == drop == reload.
 */
export function packAnchored(
  entries: readonly ProjectableEntry[],
  cols: number,
): Map<number, GridCell> {
  const out = new Map<number, GridCell>()
  if (cols < 1) return out
  const { fits, occupy, autoSlot } = makeOccupancy(cols)
  for (const e of entries) {
    const { w, h } = footprint(e, cols)
    let slot: GridCell
    if (e.col != null) {
      const c = Math.max(0, Math.min(cols - w, e.col))
      let r = 0
      while (!fits(r, c, w, h)) r++
      slot = { row: r, col: c }
    } else {
      slot = autoSlot(w, h)
    }
    occupy(slot.row, slot.col, w, h)
    out.set(e.originalIndex, slot)
  }
  return out
}

/**
 * Gravity pack of the candidate list obtained by moving the source
 * to `insertion` (index into the source-removed list) with its
 * anchor column overridden to `col`. Returns null for an out-of-
 * range insertion or a missing source.
 */
export function packWithPlacement(
  entries: readonly ProjectableEntry[],
  sourceOriginalIndex: number,
  cols: number,
  col: number,
  insertion: number,
): Map<number, GridCell> | null {
  const src = entries.find((e) => e.originalIndex === sourceOriginalIndex)
  if (!src) return null
  const rest = entries.filter((e) => e.originalIndex !== sourceOriginalIndex)
  if (insertion < 0 || insertion > rest.length) return null
  const candidate = [...rest.slice(0, insertion), { ...src, col }, ...rest.slice(insertion)]
  return packAnchored(candidate, cols)
}

/** Distance from `v` to the closed range [lo, hi]; 0 when inside. */
function outsideDistance(v: number, lo: number, hi: number): number {
  if (v < lo) return lo - v
  if (v > hi) return v - hi
  return 0
}

/** Entry-wise equality of two packed layouts. */
export function layoutsEqual(
  a: Map<number, GridCell>,
  b: Map<number, GridCell>,
): boolean {
  if (a.size !== b.size) return false
  for (const [k, cell] of a) {
    const other = b.get(k)
    if (!other || other.row !== cell.row || other.col !== cell.col) return false
  }
  return true
}

/**
 * The insertion index (into the source-removed visible list) whose
 * gravity pack, with the source's anchor column set to `pin.col`,
 * lands the source's footprint nearest the pin.
 *
 * The column is always honored exactly (it is pinned into the
 * candidate); scoring therefore effectively selects the vertical
 * slot within the pin's column band. The preview shown for the
 * returned insertion is a genuine pack output, so it is exactly what
 * a drop commits and what a reload re-derives.
 *
 * Stability rules keep the preview from fluttering between pack-
 * identical states: the current insertion wins whenever it is still
 * score-optimal, or when the best candidate produces a layout
 * identical to the current one.
 */
export function placementForPin(
  entries: readonly ProjectableEntry[],
  sourceOriginalIndex: number,
  cols: number,
  pin: GridCell,
  currentInsertion: number | null,
): number | null {
  const src = entries.find((e) => e.originalIndex === sourceOriginalIndex)
  if (!src || cols < 1) return currentInsertion
  const rest = entries.filter((e) => e.originalIndex !== sourceOriginalIndex)
  const { w, h } = footprint(src, cols)
  const pinnedSrc = { ...src, col: pin.col }

  const scores: number[] = []
  const layouts: Array<Map<number, GridCell>> = []
  for (let i = 0; i <= rest.length; i++) {
    const candidate = [...rest.slice(0, i), pinnedSrc, ...rest.slice(i)]
    const packed = packAnchored(candidate, cols)
    const p = packed.get(sourceOriginalIndex)!
    const dr = outsideDistance(pin.row, p.row, p.row + h - 1)
    const dc = outsideDistance(pin.col, p.col, p.col + w - 1)
    // Rows dominate columns in the score; the row weight only needs
    // to exceed the max column distance (< cols), so any small grid
    // is safe. With the column pinned the column term is usually 0
    // and the row term picks the vertical slot in the band.
    scores.push(dr * 64 + dc)
    layouts.push(packed)
  }

  let min = Infinity
  for (const s of scores) if (s < min) min = s

  if (currentInsertion !== null && currentInsertion < scores.length) {
    if (scores[currentInsertion] === min) return currentInsertion
  }
  const best = scores.indexOf(min)
  if (
    currentInsertion !== null &&
    currentInsertion < layouts.length &&
    layoutsEqual(layouts[best], layouts[currentInsertion])
  ) {
    return currentInsertion
  }
  return best
}
