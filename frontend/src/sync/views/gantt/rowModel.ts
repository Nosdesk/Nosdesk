/**
 * Gantt row model: which cards are scheduled, what date span each
 * bar covers, and (later) how rows group. Pure functions of
 * CardData, no pixels; the geometry projection happens in the
 * renderer through the viewport's `xOf`.
 */
import { addDays } from 'date-fns'
import type { CardData } from '@nosdesk/core/sync/views/types'
import { TERMINAL_CATEGORIES } from '@nosdesk/core/types/workflow'
import { startOfDay } from '@/composables/useGanttViewport'

export interface ScheduledCard {
  card: CardData
  start: Date
  end: Date
}

/** Non-terminal cards without a due date park in the Unscheduled
 * tray rather than fabricating a bar. */
export function isUnscheduled(card: CardData): boolean {
  const terminal = TERMINAL_CATEGORIES.has(card.workflow_state.category)
  return !terminal && !card.due_date
}

/**
 * Resolve a scheduled card's [start, end].
 *
 * Left edge: the planning `start_date` when set, else the factual
 * `created_at`. Right edge: `due_date`; a finished card without one
 * falls back to `closed_at` (then `updated_at`) so its bar reflects
 * its actual lifespan rather than stretching on every later edit.
 * End clamps to at least start + 1 day so a same-day bar has width.
 */
export function spanOf(card: CardData): { start: Date; end: Date } {
  const start = startOfDay(new Date(card.start_date ?? card.created_at))
  let end: Date
  if (card.due_date) {
    end = startOfDay(new Date(card.due_date))
  } else {
    // Only reachable for terminal cards (isUnscheduled filters
    // non-terminal cards without a due date into the tray).
    end = startOfDay(new Date(card.closed_at ?? card.updated_at))
  }
  if (end.getTime() <= start.getTime()) end = addDays(start, 1)
  return { start, end }
}

/**
 * One rendered row of the board. The lane column and the timeline
 * both iterate this list in order; `y`/`h` are the ONLY vertical
 * geometry in the board (no renderer multiplies row indexes), so
 * the two panes cannot drift.
 */
export type GanttRow =
  | {
      kind: 'group'
      key: string
      label: string
      count: number
      collapsed: boolean
      /** Min start / max end across members, for the summary span.
       * Survives collapse so the group keeps its timeline scent. */
      span: { start: Date; end: Date } | null
      y: number
      h: number
    }
  | {
      kind: 'card'
      sched: ScheduledCard
      y: number
      h: number
    }

export interface GroupedBucket {
  key: string
  label: string
  items: ScheduledCard[]
}

/**
 * Lay out rows: flat (no buckets) or grouped with collapsible
 * headers. `y` is a prefix sum, so group rows and card rows can
 * have different heights.
 */
export function buildRows(
  scheduled: ScheduledCard[],
  buckets: readonly GroupedBucket[],
  isCollapsed: (key: string) => boolean,
  rowPx: number,
  groupRowPx: number,
): { rows: GanttRow[]; totalHeight: number } {
  const rows: GanttRow[] = []
  let y = 0
  if (buckets.length === 0) {
    for (const sched of scheduled) {
      rows.push({ kind: 'card', sched, y, h: rowPx })
      y += rowPx
    }
    return { rows, totalHeight: y }
  }
  for (const bucket of buckets) {
    let span: { start: Date; end: Date } | null = null
    for (const it of bucket.items) {
      if (!span) {
        span = { start: it.start, end: it.end }
        continue
      }
      if (it.start.getTime() < span.start.getTime()) span.start = it.start
      if (it.end.getTime() > span.end.getTime()) span.end = it.end
    }
    const collapsed = isCollapsed(bucket.key)
    rows.push({
      kind: 'group',
      key: bucket.key,
      label: bucket.label,
      count: bucket.items.length,
      collapsed,
      span,
      y,
      h: groupRowPx,
    })
    y += groupRowPx
    if (!collapsed) {
      for (const sched of bucket.items) {
        rows.push({ kind: 'card', sched, y, h: rowPx })
        y += rowPx
      }
    }
  }
  return { rows, totalHeight: y }
}

/** Split a card list into canvas bars and tray items, preserving
 * input order (the shared display_order). */
export function splitSchedule(cards: readonly CardData[]): {
  scheduled: ScheduledCard[]
  unscheduled: CardData[]
} {
  const scheduled: ScheduledCard[] = []
  const unscheduled: CardData[] = []
  for (const card of cards) {
    if (isUnscheduled(card)) {
      unscheduled.push(card)
      continue
    }
    if (!card.created_at) continue
    const { start, end } = spanOf(card)
    scheduled.push({ card, start, end })
  }
  return { scheduled, unscheduled }
}
