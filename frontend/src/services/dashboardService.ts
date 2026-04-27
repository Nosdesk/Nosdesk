/**
 * Dashboard API client. Backed by Pinia Colada at the call site
 * (see `composables/useDashboardStats.ts`).
 */
import apiClient from './apiConfig'
import type { DashboardStatsGroup } from '@/views/dashboard/widgets'

export interface QueueStats {
  total: number
  unassigned: number
  open: number
  inProgress: number
  highPriority: number
  closedToday: number
}

export interface ScopedStats {
  open: number
  inProgress: number
  closed: number
  closedToday: number
  highPriority: number
}

/** Sparse bundle: only the requested groups are present. */
export interface StatsBundle {
  queue?: QueueStats
  yours?: ScopedStats
  summary?: ScopedStats
}

export interface GetStatsParams {
  /** Stat groups to compute. The backend treats omitted as "all"
   *  but our coordinator always passes an explicit list so cache
   *  keys stay stable across renders. */
  include: DashboardStatsGroup[]
  /** Override the user-scoped target (defaults to authed user). */
  user?: string
}

export async function getStats(params: GetStatsParams): Promise<StatsBundle> {
  const { data } = await apiClient.get<StatsBundle>('/dashboard/stats', {
    params: {
      include: params.include.join(','),
      user: params.user,
    },
  })
  return data
}
