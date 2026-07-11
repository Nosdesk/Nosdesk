/**
 * Batched fetch for the ticket-volume dashboard widget. The grouped
 * widget costs one request: the backend's `/dashboard/kpi-summary`
 * computes created / resolved / open in a single conditional-
 * aggregation pass on one pooled connection, rather than three
 * parallel `/kpi` calls each taking their own connection.
 */
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { useTimeRange, type Grain } from '@/composables/useTimeRange'
import { useDateStore } from '@nosdesk/core/stores/dateStore'
import {
  analyticsService,
  type KpiSummaryResult,
} from '@/services/analyticsService'

export type TicketVolumeKpis = KpiSummaryResult

/**
 * Coarsen an ISO-8601 UTC timestamp to its grain bucket, for the query
 * KEY only (the request body keeps the precise bounds). Rolling presets
 * resolve `to` to `new Date()` at millisecond precision, so keying on
 * the raw value minted a fresh cache entry every mount: the headline KPI
 * never hit the SWR cache and cold-fetched (skeleton flash) on every
 * dashboard visit. Flooring to the grain means repeat visits within the
 * same bucket share one entry. Slicing the UTC ISO is deterministic and
 * enough for a key (no timezone math needed).
 */
function keyBucket(iso: string, grain: Grain): string {
  switch (grain) {
    case 'hour':
      return iso.slice(0, 13) // YYYY-MM-DDTHH
    case 'month':
      return iso.slice(0, 7) // YYYY-MM
    case 'day':
    case 'week':
      return iso.slice(0, 10) // YYYY-MM-DD
  }
}

export function useTicketVolumeKpis() {
  const { window: timeWindow, priorWindow, compare, grain } = useTimeRange()
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
      keyBucket(params.value.from, grain.value),
      keyBucket(params.value.to, grain.value),
      params.value.prior_from ? keyBucket(params.value.prior_from, grain.value) : 'no-prior',
      params.value.prior_to ? keyBucket(params.value.prior_to, grain.value) : 'no-prior',
      params.value.tz,
    ],
    query: (): Promise<TicketVolumeKpis> => analyticsService.kpiSummary(params.value),
  })
}
