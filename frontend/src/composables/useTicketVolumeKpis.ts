/**
 * Batched fetch for the ticket-volume dashboard widget. The grouped
 * widget costs one request: the backend's `/dashboard/kpi-summary`
 * computes created / resolved / open in a single conditional-
 * aggregation pass on one pooled connection, rather than three
 * parallel `/kpi` calls each taking their own connection.
 */
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { useTimeRange } from '@/composables/useTimeRange'
import { useDateStore } from '@nosdesk/core/stores/dateStore'
import {
  analyticsService,
  type KpiSummaryResult,
} from '@/services/analyticsService'

export type TicketVolumeKpis = KpiSummaryResult

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
    query: (): Promise<TicketVolumeKpis> => analyticsService.kpiSummary(params.value),
  })
}
