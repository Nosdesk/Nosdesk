import apiClient from '../apiClient'

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
  /** When true this policy grants NO SLA to the tickets it matches (via the
   *  most-specific-wins precedence) — targets / calendar are irrelevant. */
  no_sla: boolean
  /** When the clock starts: `'created'` (from submission) or `'activated'`
   *  (from the ticket's first active state; default). */
  clock_start: string
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

export type HolidayRecurrence = 'none' | 'annual'

export interface WorkingCalendarHoliday {
  id: number
  calendar_id: number
  /** ISO date string (YYYY-MM-DD) — engine matches on the local date. */
  date: string
  /** Free-form admin-readable label like "Bank holiday". */
  label: string | null
  /** `none` (single date) or `annual` (MM-DD repeats every year). */
  recurrence: HolidayRecurrence
}

export interface WorkingCalendarHolidayBody {
  date: string
  label?: string | null
  recurrence?: HolidayRecurrence
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
  no_sla?: boolean
  clock_start?: string
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

  async listHolidays(calendarId: number): Promise<WorkingCalendarHoliday[]> {
    const { data } = await apiClient.get<WorkingCalendarHoliday[]>(
      `/admin/sla/calendars/${calendarId}/holidays`,
    )
    return data
  },
  async createHoliday(
    calendarId: number,
    body: WorkingCalendarHolidayBody,
  ): Promise<WorkingCalendarHoliday> {
    const { data } = await apiClient.post<WorkingCalendarHoliday>(
      `/admin/sla/calendars/${calendarId}/holidays`,
      body,
    )
    return data
  },
  async deleteHoliday(id: number): Promise<void> {
    await apiClient.delete(`/admin/sla/holidays/${id}`)
  },

  async explainForTicket(ticketId: number): Promise<SlaExplain> {
    const { data } = await apiClient.get<SlaExplain>(`/tickets/${ticketId}/sla/explain`)
    return data
  },

  async getPolicyMatchCounts(): Promise<Record<string, PolicyMatchCounts>> {
    const { data } = await apiClient.get<Record<string, PolicyMatchCounts>>(
      '/admin/sla/policies/matches',
    )
    return data
  },

  /** Workspace-wide roll-up of the same per-policy scan. Powers the
   *  dashboard SLA health widget so technicians + admins see the
   *  urgency signal where they're already looking. */
  async getWorkspaceSummary(): Promise<PolicyMatchCounts> {
    const { data } = await apiClient.get<PolicyMatchCounts>('/sla/workspace-summary')
    return data
  },
}

/** Per-policy state breakdown over the workspace's open tickets.
 *  Object keys are policy ids as strings (JSON map shape from
 *  Rust's HashMap<i32, _> serialisation). */
export interface PolicyMatchCounts {
  total: number
  on_track: number
  at_risk: number
  breached: number
  paused: number
}

// "Why this SLA?" payload returned by GET /api/tickets/{id}/sla/explain.
// Surfaces the matched policy, its calendar, the workflow-state pause
// flag, and the typed filters the matcher accepted as hits.
export interface SlaExplain {
  policy: SlaExplainPolicy | null
  state: SlaExplainState
}

export interface SlaExplainPolicy {
  id: number
  name: string
  is_default: boolean
  /** When true the matched policy grants no SLA (targets/calendar irrelevant). */
  no_sla: boolean
  /** `'created'` or `'activated'` — when the matched policy's clock starts. */
  clock_start: string
  target_response_minutes: number | null
  target_resolution_minutes: number | null
  calendar: SlaExplainCalendar | null
  matched_filters: SlaExplainFilter[]
}

export interface SlaExplainCalendar {
  id: number
  name: string
  timezone: string
}

export interface SlaExplainState {
  paused: boolean
  state_name: string
}

export type SlaExplainFilter =
  | { kind: 'priority'; value: string }
  | { kind: 'category'; id: number; name: string }
  | { kind: 'assignee_group'; id: number; name: string }
