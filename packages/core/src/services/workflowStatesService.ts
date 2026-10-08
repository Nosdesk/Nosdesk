import apiClient from '../apiClient'
import type { WorkflowState, WorkflowStateCategory } from '../types/workflow'

interface ListResponse {
  states: WorkflowState[]
}

export interface CreateWorkflowStateBody {
  name: string
  category: WorkflowStateCategory
  color: string
  /**
   * Optional override; the backend derives a default from the
   * category when omitted (active = clock running, every other
   * category pauses).
   */
  pauses_sla?: boolean
}

export interface UpdateWorkflowStateBody {
  name?: string
  color?: string
  position?: number
  is_default?: boolean
  pauses_sla?: boolean
}

export const workflowStatesService = {
  async list(): Promise<WorkflowState[]> {
    const { data } = await apiClient.get<ListResponse>('/workflow-states')
    return data.states
  },

  async create(body: CreateWorkflowStateBody): Promise<WorkflowState> {
    const { data } = await apiClient.post<WorkflowState>('/admin/workflow-states', body)
    return data
  },

  async update(id: number, body: UpdateWorkflowStateBody): Promise<WorkflowState> {
    const { data } = await apiClient.patch<WorkflowState>(
      `/admin/workflow-states/${id}`,
      body,
    )
    return data
  },

  async archive(id: number): Promise<WorkflowState> {
    const { data } = await apiClient.delete<WorkflowState>(`/admin/workflow-states/${id}`)
    return data
  },

  /** Archived states (admin). Tickets in them keep them. */
  async listArchived(): Promise<WorkflowState[]> {
    const { data } = await apiClient.get<ListResponse>('/admin/workflow-states/archived')
    return data.states
  },

  /** Bring an archived state back, at the end of its category (admin). */
  async restore(id: number): Promise<WorkflowState> {
    const { data } = await apiClient.post<WorkflowState>(`/admin/workflow-states/${id}/restore`)
    return data
  },
}
