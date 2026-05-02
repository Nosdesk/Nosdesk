import apiClient from './apiConfig'
import type { WorkflowState, WorkflowStateCategory } from '@/types/workflow'

interface ListResponse {
  states: WorkflowState[]
}

export interface CreateWorkflowStateBody {
  name: string
  category: WorkflowStateCategory
  color: string
}

export interface UpdateWorkflowStateBody {
  name?: string
  color?: string
  position?: number
  is_default?: boolean
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
}
