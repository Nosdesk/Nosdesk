import apiClient from './apiConfig'

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

export const cyclesService = {
  async list(projectId: number): Promise<Cycle[]> {
    const { data } = await apiClient.get<Cycle[]>(`/projects/${projectId}/cycles`)
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

  async addTicket(cycleUuid: string, ticketId: number): Promise<void> {
    await apiClient.post(`/cycles/${cycleUuid}/tickets/${ticketId}`)
  },

  async removeTicket(cycleUuid: string, ticketId: number): Promise<void> {
    await apiClient.delete(`/cycles/${cycleUuid}/tickets/${ticketId}`)
  },
}
