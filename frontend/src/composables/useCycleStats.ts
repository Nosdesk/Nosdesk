/**
 * Per-cycle progress derived from the ticket pool.
 *
 * The workspace pool carries every ticket with a denormalised
 * `cycle_id` (kept live by the backend's `ticket.cycle_changed`
 * partial op-U), so live cycles fold their counts straight off the
 * pool with no REST call. Completed cycles read their frozen
 * `completion_snapshot` instead, so post-completion ticket edits
 * never move historical numbers.
 */
import { computed, type ComputedRef } from 'vue'
import type { SyncTicket } from '@/sync/stores/tickets'
import { TERMINAL_CATEGORIES } from '@nosdesk/core/types/workflow'

export interface CycleProgress {
  completed: number
  total: number
  by_category: Record<string, number>
  /** Frozen-only extras (undefined on live cycles). */
  carried_over?: number
  scope_added?: number
  frozen_at?: string
}

interface CycleLike {
  id: number
  state: string
  completion_snapshot?: Record<string, unknown> | null
}

export function useCycleStats(tickets: ComputedRef<SyncTicket[]>): {
  statsFor: (cycle: CycleLike) => CycleProgress
  pctFor: (cycle: CycleLike) => number
} {
  const liveByCycle = computed(() => {
    const map = new Map<number, CycleProgress>()
    for (const ticket of tickets.value) {
      if (ticket.cycle_id == null) continue
      let s = map.get(ticket.cycle_id)
      if (!s) {
        s = { completed: 0, total: 0, by_category: {} }
        map.set(ticket.cycle_id, s)
      }
      s.total++
      const cat = ticket.workflow_state?.category
      if (cat) {
        s.by_category[cat] = (s.by_category[cat] ?? 0) + 1
        if (TERMINAL_CATEGORIES.has(cat)) s.completed++
      }
    }
    return map
  })

  function statsFor(cycle: CycleLike): CycleProgress {
    if (cycle.state === 'completed' && cycle.completion_snapshot) {
      const snap = cycle.completion_snapshot
      return {
        completed: Number(snap.completed ?? 0),
        total: Number(snap.tickets ?? 0),
        by_category: (snap.by_category as Record<string, number>) ?? {},
        carried_over: snap.carried_over != null ? Number(snap.carried_over) : undefined,
        scope_added: snap.scope_added != null ? Number(snap.scope_added) : undefined,
        frozen_at: typeof snap.frozen_at === 'string' ? snap.frozen_at : undefined,
      }
    }
    return liveByCycle.value.get(cycle.id) ?? { completed: 0, total: 0, by_category: {} }
  }

  function pctFor(cycle: CycleLike): number {
    const s = statsFor(cycle)
    return s.total > 0 ? Math.round((s.completed / s.total) * 100) : 0
  }

  return { statsFor, pctFor }
}
