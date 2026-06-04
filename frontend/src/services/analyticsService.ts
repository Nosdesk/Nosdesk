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
  /** IANA timezone the sparkline's daily buckets align to (the user's
   *  effective zone). Omitted / invalid = UTC on the backend. Keeps
   *  each day's bucket on the user's local day boundary. */
  tz?: string
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
  /** Bucket granularity. `hour` for the today view (24 hourly
   *  points), `day` otherwise. Omitted = day on the backend. */
  grain?: 'hour' | 'day'
  /** IANA timezone the buckets align to (the user's effective zone).
   *  Omitted / invalid = UTC on the backend. Keeps "today" hourly
   *  buckets on the user's local hours. */
  tz?: string
}

export type BreakdownGroupBy = 'priority' | 'category' | 'assignee'

export interface BreakdownBucket {
  key: string
  value: number
}

export interface BreakdownResult {
  buckets: BreakdownBucket[]
}

export interface BreakdownParams {
  /** The wire name aliases through the backend: `category_id` ->
   *  `category`, `assignee_uuid` -> `assignee`. The frontend uses
   *  the shorter forms. */
  group_by: BreakdownGroupBy
  from: string
  to: string
  top_n?: number
}

export interface HeatmapCell {
  /** Day-of-week, 0 = Sunday through 6 = Saturday (Postgres EXTRACT(dow)). */
  dow: number
  /** Hour-of-day, 0..=23. */
  hour: number
  value: number
}

export interface HeatmapResult {
  cells: HeatmapCell[]
}

export interface HeatmapParams {
  from: string
  to: string
}

export type LeaderboardActor = 'assignee' | 'requester'

export interface LeaderboardRow {
  actor_uuid: string | null
  value: number
}

export interface LeaderboardResult {
  rows: LeaderboardRow[]
}

export interface LeaderboardParams {
  actor: LeaderboardActor
  from: string
  to: string
  top_n?: number
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
  async breakdown(params: BreakdownParams): Promise<BreakdownResult> {
    const { data } = await apiClient.get<BreakdownResult>('/dashboard/breakdown', { params })
    return data
  },
  async heatmap(params: HeatmapParams): Promise<HeatmapResult> {
    const { data } = await apiClient.get<HeatmapResult>('/dashboard/heatmap', { params })
    return data
  },
  async leaderboard(params: LeaderboardParams): Promise<LeaderboardResult> {
    const { data } = await apiClient.get<LeaderboardResult>('/dashboard/leaderboard', { params })
    return data
  },
}
