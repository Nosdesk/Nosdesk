/**
 * Batched fetch for the ticket-volume dashboard widget. One query
 * loads created / resolved / open in parallel so the grouped widget
 * costs a single round-trip instead of three separate KpiTile mounts.
 */
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { useTimeRange } from '@/composables/useTimeRange'
import { useDateStore } from '@/stores/dateStore'
import {
  analyticsService,
  type KpiMetric,
  type KpiResult,
} from '@/services/analyticsService'

export interface TicketVolumeKpis {
  created: KpiResult
  resolved: KpiResult
  open: KpiResult
}

const METRICS: KpiMetric[] = ['tickets_created', 'tickets_resolved', 'tickets_open']

export function useTicketVolumeKpis() {
  const { window: timeWindow, priorWindow, compare } = useTimeRange()
  const dateStore = useDateStore()

  const params = computed(() => {
    const w = timeWindow.value
    const includePrior = compare.value
    const p = includePrior ? priorWindow.value : undefined
    return {
      from: w.from,
      to: w.to,
      prior_from: p?.from,
      prior_to: p?.to,
      tz: dateStore.effectiveTimezone,
    }
  })

  return useQuery({
    key: () => [
      'dashboard',
      'kpi',
      'ticket-volume',
      params.value.from,
      params.value.to,
      params.value.prior_from ?? 'no-prior',
      params.value.prior_to ?? 'no-prior',
      params.value.tz,
    ],
    query: async (): Promise<TicketVolumeKpis> => {
      const base = params.value
      const [created, resolved, open] = await Promise.all(
        METRICS.map((metric) =>
          analyticsService.kpi({
            ...base,
            metric,
            sparkline: metric !== 'tickets_open',
          }),
        ),
      )
      return { created, resolved, open }
    },
  })
}
