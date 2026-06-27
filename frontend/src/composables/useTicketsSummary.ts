/**
 * Per-queue summary stats for the tickets header. Computes the
 * "12 open · 3 paused · 2 breached" line that gives a tech an
 * immediate read on what they're walking into.
 *
 * Reads from the post-view-filter card set (not the post-chip
 * set) so the numbers describe the queue itself, not the
 * temporarily filtered view of it. That's what users actually
 * care about: "how big is My Queue?" — not "how big is My
 * Queue right now while I'm hiding the closed ones."
 */
import { computed, type ComputedRef } from 'vue'
import { TERMINAL_CATEGORIES } from '@nosdesk/core/types/workflow'
import type { CardData } from '@nosdesk/core/sync/views/types'

export interface QueueSummary {
  total: number
  open: number
  closed: number
  paused: number
  breached: number
  unassigned: number
}

export interface UseTicketsSummary {
  summary: ComputedRef<QueueSummary>
  /** Compact display segments — only includes the categories
   * that have a non-zero count, so we don't render "0 paused"
   * noise. Each segment is a tone-tagged label so the header
   * can colour critical numbers (breached) differently. */
  segments: ComputedRef<{ label: string; tone: 'default' | 'amber' | 'red' }[]>
}

export function useTicketsSummary(cards: ComputedRef<CardData[]>): UseTicketsSummary {
  const summary = computed<QueueSummary>(() => {
    let open = 0
    let closed = 0
    let paused = 0
    let breached = 0
    let unassigned = 0
    for (const c of cards.value) {
      if (TERMINAL_CATEGORIES.has(c.workflow_state.category)) closed++
      else open++
      if (c.sla?.paused) paused++
      if (c.sla?.breached) breached++
      if (!c.assignee_uuid) unassigned++
    }
    return { total: cards.value.length, open, closed, paused, breached, unassigned }
  })

  const segments = computed<{ label: string; tone: 'default' | 'amber' | 'red' }[]>(() => {
    const s = summary.value
    const out: { label: string; tone: 'default' | 'amber' | 'red' }[] = []
    out.push({ label: `${s.open} open`, tone: 'default' })
    if (s.breached > 0) out.push({ label: `${s.breached} breached`, tone: 'red' })
    if (s.paused > 0) out.push({ label: `${s.paused} paused`, tone: 'amber' })
    if (s.unassigned > 0) out.push({ label: `${s.unassigned} unassigned`, tone: 'default' })
    return out
  })

  return { summary, segments }
}
