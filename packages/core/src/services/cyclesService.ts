import apiClient from '../apiClient'

export interface Cycle {
  id: number
  uuid: string
  project_id: number
  name: string
  start_at: string | null
  end_at: string | null
  state: 'planned' | 'active' | 'completed'
  completion_snapshot: Record<string, unknown> | null
  completed_at: string | null
  created_at: string
  updated_at: string
  archived_at: string | null
  created_by: string | null
}

export interface CreateCycleBody {
  name: string
  start_at?: string | null
  end_at?: string | null
  state?: 'planned' | 'active'
}

export interface UpdateCycleBody {
  name?: string
  start_at?: string | null
  end_at?: string | null
  state?: 'planned' | 'active'
}

/** Stats payload returned by GET /cycles/{uuid}/stats. Matches the
 * frozen completion_snapshot shape so the widget renders both
 * planned/active (live) and completed (frozen) cycles through one
 * code path. */
export interface CycleStats {
  frozen_at: string
  tickets: number
  completed: number
  by_category: Record<string, number>
  /** Tickets that were still open when the cycle completed and were
   * moved to the next cycle (or unlinked to the backlog). Only
   * present on frozen completion snapshots. */
  carried_over?: number
  /** Tickets added to the cycle after its start date (mid-cycle scope
   * creep). Computed live for active cycles, frozen on completion. */
  scope_added?: number
}

/** Count-based burnup series returned by GET /cycles/{uuid}/burnup.
 * Reconstructed from member add times (scope) and ticket close times
 * (completed); no daily-rollup table backs it. Empty points when the
 * cycle has no start_at. */
export interface BurnupSeries {
  start: string | null
  end: string | null
  final_scope: number
  /** Scope committed by the start date; final_scope minus this is the
   * mid-cycle creep, drawn as a baseline on the chart. */
  start_scope: number
  points: { day: string; scope: number; completed: number }[]
}

export const cyclesService = {
  async list(projectId: number): Promise<Cycle[]> {
    const { data } = await apiClient.get<Cycle[]>(`/projects/${projectId}/cycles`)
    return data
  },

  /** Workspace-wide list. Defaults to active + planned at the
   * server; pass an explicit `state` (comma-separated) to opt
   * `completed` back in. */
  async listWorkspace(states?: Cycle['state'][]): Promise<Cycle[]> {
    const params = states && states.length ? { state: states.join(',') } : {}
    const { data } = await apiClient.get<Cycle[]>('/cycles', { params })
    return data
  },

  async get(uuid: string): Promise<Cycle> {
    const { data } = await apiClient.get<Cycle>(`/cycles/${uuid}`)
    return data
  },

  async create(projectId: number, body: CreateCycleBody): Promise<Cycle> {
    const { data } = await apiClient.post<Cycle>(`/projects/${projectId}/cycles`, body)
    return data
  },

  async update(uuid: string, body: UpdateCycleBody): Promise<Cycle> {
    const { data } = await apiClient.patch<Cycle>(`/cycles/${uuid}`, body)
    return data
  },

  async complete(uuid: string): Promise<Cycle> {
    const { data } = await apiClient.post<Cycle>(`/cycles/${uuid}/complete`, {})
    return data
  },

  async archive(uuid: string): Promise<void> {
    await apiClient.delete(`/cycles/${uuid}`)
  },

  async stats(uuid: string): Promise<CycleStats> {
    const { data } = await apiClient.get<CycleStats>(`/cycles/${uuid}/stats`)
    return data
  },

  async burnup(uuid: string): Promise<BurnupSeries> {
    const { data } = await apiClient.get<BurnupSeries>(`/cycles/${uuid}/burnup`)
    return data
  },

  /** Returns the ticket-id list for a cycle. ScrumBoard uses this
   * to scope its kanban without pulling cycle_tickets through the
   * sync engine. */
  async tickets(uuid: string): Promise<number[]> {
    const { data } = await apiClient.get<number[]>(`/cycles/${uuid}/tickets`)
    return data
  },

  async addTicket(cycleUuid: string, ticketId: number): Promise<void> {
    await apiClient.post(`/cycles/${cycleUuid}/tickets/${ticketId}`)
  },

  async removeTicket(cycleUuid: string, ticketId: number): Promise<void> {
    await apiClient.delete(`/cycles/${cycleUuid}/tickets/${ticketId}`)
  },
}
