import apiClient from './apiConfig'

export interface WorkingCalendar {
  id: number
  name: string
  timezone: string
  schedule: Record<string, [string, string][]>
  is_default: boolean
  created_at: string
  updated_at: string
  created_by: string | null
}

export interface SlaPolicy {
  id: number
  name: string
  target_response_minutes: number | null
  target_resolution_minutes: number | null
  working_calendar_id: number | null
  priority_filter: string | null
  category_id_filter: number | null
  assignee_group_id_filter: number | null
  is_default: boolean
  created_at: string
  updated_at: string
  created_by: string | null
}

export interface WorkingCalendarBody {
  name: string
  timezone?: string
  schedule: Record<string, [string, string][]>
  is_default?: boolean
}

export interface SlaPolicyBody {
  name: string
  target_response_minutes?: number | null
  target_resolution_minutes?: number | null
  working_calendar_id?: number | null
  priority_filter?: string | null
  category_id_filter?: number | null
  assignee_group_id_filter?: number | null
  is_default?: boolean
}

export const slaService = {
  async listPolicies(): Promise<SlaPolicy[]> {
    const { data } = await apiClient.get<SlaPolicy[]>('/admin/sla/policies')
    return data
  },
  async createPolicy(body: SlaPolicyBody): Promise<SlaPolicy> {
    const { data } = await apiClient.post<SlaPolicy>('/admin/sla/policies', body)
    return data
  },
  async updatePolicy(id: number, body: SlaPolicyBody): Promise<SlaPolicy> {
    const { data } = await apiClient.patch<SlaPolicy>(`/admin/sla/policies/${id}`, body)
    return data
  },
  async deletePolicy(id: number): Promise<void> {
    await apiClient.delete(`/admin/sla/policies/${id}`)
  },

  async listCalendars(): Promise<WorkingCalendar[]> {
    const { data } = await apiClient.get<WorkingCalendar[]>('/admin/sla/calendars')
    return data
  },
  async createCalendar(body: WorkingCalendarBody): Promise<WorkingCalendar> {
    const { data } = await apiClient.post<WorkingCalendar>('/admin/sla/calendars', body)
    return data
  },
  async updateCalendar(id: number, body: WorkingCalendarBody): Promise<WorkingCalendar> {
    const { data } = await apiClient.patch<WorkingCalendar>(`/admin/sla/calendars/${id}`, body)
    return data
  },
  async deleteCalendar(id: number): Promise<void> {
    await apiClient.delete(`/admin/sla/calendars/${id}`)
  },
}
