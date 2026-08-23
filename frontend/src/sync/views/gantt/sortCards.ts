/**
 * Row ordering for the gantt. The vertical axis carries no meaning of its
 * own (time is the horizontal axis), so rows follow a RULE rather than a
 * curated order: rules maintain themselves as dates shift, where a manual
 * order goes stale and becomes grooming work. Sorting the flat card list
 * is enough for grouped mode too — the grouping projector buckets in
 * input order, so each group inherits the sort.
 */
import type { CardData } from '@nosdesk/core/sync/views/types'
import { isUnscheduled, spanOf } from './rowModel'

export type GanttSortKey = 'start' | 'due' | 'priority'

export const GANTT_SORT_KEYS: readonly GanttSortKey[] = ['start', 'due', 'priority']

/** Urgent first; cards with no priority set sink to the bottom. */
const PRIORITY_RANK: Record<CardData['priority'], number> = {
  urgent: 0,
  high: 1,
  medium: 2,
  low: 3,
  none: 4,
}

/**
 * Sort key per card. `start`/`due` rank by the same resolved span the bars
 * are drawn from (so the order always matches what the eye sees), with the
 * tray's unscheduled cards ranked last under `due` — they have none.
 */
function rankOf(card: CardData, key: GanttSortKey): number {
  switch (key) {
    case 'start':
      return spanOf(card).start.getTime()
    case 'due':
      return isUnscheduled(card) ? Number.POSITIVE_INFINITY : spanOf(card).end.getTime()
    case 'priority':
      return PRIORITY_RANK[card.priority] ?? PRIORITY_RANK.none
  }
}

/** Stable: ties keep the input (shared project) order. */
export function sortCards(cards: readonly CardData[], key: GanttSortKey): CardData[] {
  return [...cards].sort((a, b) => rankOf(a, key) - rankOf(b, key))
}
