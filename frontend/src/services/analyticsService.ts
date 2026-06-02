/**
 * Dashboard analytics API client. Wraps the Phase 4 endpoints
 * (`/api/dashboard/kpi` and `/api/dashboard/timeseries`) plus the
 * Wave 5+ endpoints as they ship. The wire shapes mirror the typed
 * structs in `backend/src/repository/analytics.rs`; the union
 * vocabulary on each *Metric / *Measure / *TimeField type is the
 * canonical enum.
 */
import apiClient from './apiConfig'

export type KpiMetric = 'tickets_created' | 'tickets_resolved' | 'tickets_open'

export interface KpiResult {
  value: number
  /** Difference from the prior period; `null` for snapshot metrics
   *  or when no prior window was supplied. */
  delta_value: number | null
  /** Percent change from the prior period (one decimal). `null`
   *  when prior is zero (undefined) or absent. */
  delta_pct: number | null
  /** Per-day series for the sparkline. `null` when `sparkline=false`
   *  was requested or the metric is a snapshot. */
  sparkline: number[] | null
}

export interface KpiParams {
  metric: KpiMetric
  /** ISO-8601 (UTC); use `useTimeRange.presetWindow` to build. */
  from: string
  to: string
  prior_from?: string
  prior_to?: string
  /** Default true; pass false on dense renderers that don't show a
   *  sparkline alongside the number. */
  sparkline?: boolean
}

export type TsMeasure = 'count'
export type TsTimeField = 'created_at' | 'closed_at' | 'resolved_at'

export interface TimeseriesBucket {
  /** ISO-8601 day boundary (UTC). */
  ts: string
  value: number
}

export interface TimeseriesResult {
  buckets: TimeseriesBucket[]
}

export interface TimeseriesParams {
  measure: TsMeasure
  time_field: TsTimeField
  from: string
  to: string
}

export const analyticsService = {
  async kpi(params: KpiParams): Promise<KpiResult> {
    const { data } = await apiClient.get<KpiResult>('/dashboard/kpi', { params })
    return data
  },
  async timeseries(params: TimeseriesParams): Promise<TimeseriesResult> {
    const { data } = await apiClient.get<TimeseriesResult>('/dashboard/timeseries', { params })
    return data
  },
}
