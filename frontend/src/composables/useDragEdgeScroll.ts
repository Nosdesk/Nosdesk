/** Edge-proximity auto-scroll while dragging. */

export interface DragEdgeScrollTarget {
  el: HTMLElement
  axes: 'x' | 'y' | 'both'
}

/** Pixels from the viewport edge where auto-scroll begins. Scaled with EDGE_MAX
 *  so the acceleration ramp feels the same at higher speeds. */
const EDGE_BAND = 180
/** Maximum scroll velocity (px per frame at ~60fps). */
const EDGE_MAX = 12
/** Cap the band at a fraction of the axis so a narrow viewport keeps a neutral
 *  middle. At a flat 180px on a 390px phone the two bands leave a 30px gap, so
 *  nearly every drag auto-pans and the board feels like it is fleeing the
 *  finger. 20% gives ~78px bands there and is a no-op on desktop, where 180px
 *  is already well under the cap. */
const EDGE_BAND_MAX_FRACTION = 0.2

/** The band actually used for an axis of the given length. */
export function bandForAxis(axisLength: number, band = EDGE_BAND): number {
  return Math.min(band, Math.floor(axisLength * EDGE_BAND_MAX_FRACTION))
}

export function scrollDeltaForEdge(
  pointer: number,
  edgeMin: number,
  edgeMax: number,
  band = EDGE_BAND,
  maxSpeed = EDGE_MAX,
): number {
  if (pointer < edgeMin + band) {
    const ratio = 1 - Math.max(0, pointer - edgeMin) / band
    return -Math.round(maxSpeed * ratio)
  }
  if (pointer > edgeMax - band) {
    const ratio = 1 - Math.max(0, edgeMax - pointer) / band
    return Math.round(maxSpeed * ratio)
  }
  return 0
}

/** Discover scroll containers relevant to ticket drags at (x, y). */
export function getDefaultDragScrollTargets(
  clientX: number,
  clientY: number,
): DragEdgeScrollTarget[] {
  const targets: DragEdgeScrollTarget[] = []
  const seen = new Set<HTMLElement>()

  const add = (el: HTMLElement | null, axes: DragEdgeScrollTarget['axes']) => {
    if (!el || seen.has(el)) return
    seen.add(el)
    targets.push({ el, axes })
  }

  const board = document.querySelector<HTMLElement>('.kanban-board')

  function boardScrollTarget(): HTMLElement | null {
    if (!board) return null
    const style = getComputedStyle(board)
    const scrollsY = style.overflowY === 'auto' || style.overflowY === 'scroll'
    if (scrollsY && board.scrollHeight > board.clientHeight) return board
    return null
  }

  if (board) {
    if (board.scrollWidth > board.clientWidth) add(board, 'x')
    add(boardScrollTarget(), 'y')
  }

  // Legacy: inner lane scrollports (if any remain).
  function laneScrollBody(col: HTMLElement): HTMLElement | null {
    const style = getComputedStyle(col)
    if (
      (style.overflowY === 'auto' || style.overflowY === 'scroll')
      && col.scrollHeight > col.clientHeight
    ) {
      return col
    }
    const inner = col.querySelector<HTMLElement>('.overflow-y-auto')
    if (inner && inner.scrollHeight > inner.clientHeight) return inner
    return null
  }

  if (!targets.some((t) => t.axes === 'y')) {
    const stack = document.elementsFromPoint(clientX, clientY)
    for (const el of stack) {
      if (!(el instanceof HTMLElement)) continue
      const col = el.closest('[data-column-id]')
      if (col instanceof HTMLElement) {
        add(laneScrollBody(col), 'y')
        break
      }
      if (
        el.classList.contains('overflow-y-auto')
        && el.scrollHeight > el.clientHeight
      ) {
        add(el, 'y')
      }
    }
  }

  if (!targets.some((t) => t.axes === 'y') && board) {
    for (const col of board.querySelectorAll<HTMLElement>('[data-column-id]')) {
      const rect = col.getBoundingClientRect()
      if (clientX < rect.left || clientX > rect.right) continue
      add(laneScrollBody(col), 'y')
      break
    }
  }

  return targets
}

export function createDragEdgeScroller(options?: {
  getTargets?: (clientX: number, clientY: number) => DragEdgeScrollTarget[]
  onTick?: (clientX: number, clientY: number) => void
  band?: number
  maxSpeed?: number
}) {
  let rafId = 0
  let active = false
  let pointer = { x: 0, y: 0 }
  const band = options?.band ?? EDGE_BAND
  const maxSpeed = options?.maxSpeed ?? EDGE_MAX
  const getTargets = options?.getTargets ?? getDefaultDragScrollTargets

  function tick(): void {
    rafId = 0
    if (!active) return

    const { x, y } = pointer
    const viewW = window.innerWidth
    const viewH = window.innerHeight

    for (const { el, axes } of getTargets(x, y)) {
      if (!el.isConnected) continue
      if (axes === 'x' || axes === 'both') {
        const dx = scrollDeltaForEdge(x, 0, viewW, bandForAxis(viewW, band), maxSpeed)
        if (dx !== 0 && el.scrollWidth > el.clientWidth) {
          el.scrollLeft += dx
        }
      }
      if (axes === 'y' || axes === 'both') {
        const dy = scrollDeltaForEdge(y, 0, viewH, bandForAxis(viewH, band), maxSpeed)
        if (dy !== 0 && el.scrollHeight > el.clientHeight) {
          el.scrollTop += dy
        }
      }
    }

    options?.onTick?.(x, y)
    rafId = requestAnimationFrame(tick)
  }

  function scheduleTick(): void {
    if (rafId !== 0) return
    rafId = requestAnimationFrame(tick)
  }

  return {
    start(): void {
      active = true
      scheduleTick()
    },
    stop(): void {
      active = false
      if (rafId) {
        cancelAnimationFrame(rafId)
        rafId = 0
      }
    },
    update(clientX: number, clientY: number): void {
      pointer = { x: clientX, y: clientY }
      if (!active) return
      scheduleTick()
    },
  }
}
