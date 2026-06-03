<!--
Drop-position indicator for the dashboard widget drag-and-drop.
Renders a 2px accent line at the PROJECTED post-drop slot — not at
the frozen pre-drop gap. The gap shape comes from
`computeDropTargetGap` which performs the same array splice the
store will run on commit, so the line marks the visual landing
spot rather than an index-space gap.

Position math runs against the grid container's measured bounding
rect, using the rendered column count snapshotted at drag-start.
That collapses cleanly on the `grid-cols-1 xl:grid-cols-3` layout
without a separate mobile code path: below xl, every widget renders
at span-1 and the indicator becomes a full-width line.

No transition on enter, exit, or prop change. The indicator is a
state readout — sibling reflow is the animated part (handled by
the existing FLIP transition on the grid).
-->
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { DropTargetGap } from '@/composables/usePointerSortable'

const props = defineProps<{
  gap: DropTargetGap
  gridEl: HTMLElement | null
  /** Snapshotted at drag-start. The grid container's
   *  `grid-template-columns` may report different values across
   *  viewport breakpoints, but we lock it for the gesture so a
   *  resize mid-drag doesn't fragment the math. */
  renderedColumns: number
}>()

interface IndicatorBox {
  top: number
  left: number
  width: number
  suppressLeftTerminal: boolean
}

const box = ref<IndicatorBox | null>(null)

/** 4px bracket affordance that bleeds the indicator's ends outside
 *  the grid's left edge. Suppressed when any ancestor clips its
 *  overflow, so the bleed doesn't get sliced. */
const TERMINAL_BLEED = 4

function hasClippedAncestor(el: HTMLElement): boolean {
  let node: HTMLElement | null = el.parentElement
  while (node) {
    const overflow = getComputedStyle(node).overflow
    if (overflow === 'hidden' || overflow === 'clip') return true
    node = node.parentElement
  }
  return false
}

function measureGridMetrics(grid: HTMLElement) {
  const rect = grid.getBoundingClientRect()
  const style = getComputedStyle(grid)
  const gapPx = parseFloat(style.rowGap || style.gap || '0') || 0
  const colGapPx = parseFloat(style.columnGap || style.gap || '0') || 0
  const cols = Math.max(1, props.renderedColumns)
  const totalColGaps = (cols - 1) * colGapPx
  const columnWidth = (rect.width - totalColGaps) / cols
  return { rect, gapPx, colGapPx, columnWidth }
}

function recompute() {
  const grid = props.gridEl
  if (!grid) {
    box.value = null
    return
  }
  const { gap } = props
  const { rect, gapPx, colGapPx, columnWidth } = measureGridMetrics(grid)

  // Vertical position: top of the target row, MINUS half the row gap
  // so the line sits in the middle of the gap between rows. Row
  // heights vary, so we walk the actual children grouped by row.
  const rows = collectRowTops(grid)
  let topPx: number
  if (rows.length === 0) {
    topPx = rect.top
  } else if (gap.rowIndex >= rows.length) {
    // Trailing append: line sits half-a-gap below the last row's bottom.
    const last = rows[rows.length - 1]
    topPx = last.bottom + gapPx / 2
  } else {
    const row = rows[gap.rowIndex]
    topPx = row.top - gapPx / 2
  }

  const colStartIdx = Math.max(0, gap.columnStart - 1)
  const colEndExclusive = Math.min(props.renderedColumns, colStartIdx + gap.colSpan)
  const cellCount = Math.max(1, colEndExclusive - colStartIdx)
  const left = rect.left + colStartIdx * (columnWidth + colGapPx)
  const width = cellCount * columnWidth + (cellCount - 1) * colGapPx

  const suppressLeftTerminal = hasClippedAncestor(grid)
  const finalLeft = suppressLeftTerminal ? left : left - TERMINAL_BLEED
  const finalWidth = suppressLeftTerminal
    ? width + TERMINAL_BLEED
    : width + TERMINAL_BLEED * 2

  box.value = {
    top: topPx,
    left: finalLeft,
    width: finalWidth,
    suppressLeftTerminal,
  }
}

interface RowBounds {
  top: number
  bottom: number
}

/** Walk the grid's direct children, grouping them into rows by
 *  rounded top offset. Returns row top/bottom in viewport pixels.
 *  Sortable-indexed children only (the indicator itself isn't
 *  marked, so it doesn't pollute the row grouping). */
function collectRowTops(grid: HTMLElement): RowBounds[] {
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
  return rows
}

watch(
  () => [props.gap.columnStart, props.gap.rowIndex, props.gap.colSpan, props.gridEl],
  () => recompute(),
  { immediate: true },
)

function onResize() {
  recompute()
}

onMounted(() => {
  window.addEventListener('resize', onResize)
  window.addEventListener('scroll', onResize, { passive: true })
})

onBeforeUnmount(() => {
  window.removeEventListener('resize', onResize)
  window.removeEventListener('scroll', onResize)
})

const styleObject = computed(() => {
  if (!box.value) return null
  return {
    top: `${box.value.top}px`,
    left: `${box.value.left}px`,
    width: `${box.value.width}px`,
  }
})
</script>

<template>
  <Teleport to="body">
    <div
      v-if="styleObject"
      :class="[
        'fixed h-0.5 bg-accent rounded-sm pointer-events-none z-20',
        box?.suppressLeftTerminal ? 'drop-indicator drop-indicator--no-left-bleed' : 'drop-indicator',
      ]"
      :style="styleObject"
      aria-hidden="true"
    />
  </Teleport>
</template>

<style scoped>
/* 8px diameter rounded terminals that bracket the indicator line.
 * The pseudo-elements pin against the line ends; the line itself
 * already bleeds 4px past each grid edge in the parent's position
 * math (suppressed for the left when an ancestor clips overflow). */
.drop-indicator::before,
.drop-indicator::after {
  content: '';
  position: absolute;
  top: 50%;
  width: 8px;
  height: 8px;
  border-radius: 9999px;
  background: var(--color-accent);
  transform: translateY(-50%);
}
.drop-indicator::before {
  left: 0;
}
.drop-indicator::after {
  right: 0;
}
.drop-indicator--no-left-bleed::before {
  display: none;
}
</style>
