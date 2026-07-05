/**
 * Burnup series for a cycle via Pinia Colada. The daily series is
 * the one cycle read that stays server-side (it replays the event
 * log); everything else derives from the sync pool. Cache-first
 * with silent revalidate, and a live membership change invalidates
 * the entry so the chart redraws without a revisit.
 */
import { computed, toValue, type MaybeRefOrGetter } from 'vue'
import { useQuery, useQueryCache } from '@pinia/colada'
import { cyclesService, type BurnupSeries } from '@nosdesk/core/services/cyclesService'
import { useSyncActions } from '@/composables/useSyncActions'

export function useCycleBurnup(
  uuid: MaybeRefOrGetter<string | null>,
  enabled: MaybeRefOrGetter<boolean> = true,
) {
  const queryCache = useQueryCache()

  const query = useQuery({
    key: () => ['cycle', toValue(uuid) ?? '', 'burnup'],
    query: () => cyclesService.burnup(toValue(uuid) as string),
    enabled: () => !!toValue(uuid) && toValue(enabled),
  })

  // Membership and close events move the series; invalidate on the
  // relevant sync actions (debounced) so an open chart stays honest.
  useSyncActions(
    (actions) => {
      const relevant = actions.some(
        (a) =>
          a.event_type === 'ticket.cycle_changed' ||
          a.event_type === 'ticket.workflow_state_changed' ||
          a.aggregate === 'cycle_ticket',
      )
      const u = toValue(uuid)
      if (relevant && u) {
        void queryCache.invalidateQueries({ key: ['cycle', u, 'burnup'] })
      }
    },
    { aggregates: ['ticket', 'cycle_ticket'], debounceMs: 500 },
  )

  return {
    burnup: computed<BurnupSeries | null>(() => query.data.value ?? null),
  }
}
