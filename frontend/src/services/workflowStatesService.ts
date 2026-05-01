import apiClient from './apiConfig'
import type { WorkflowState } from '@/types/workflow'

interface ListResponse {
  states: WorkflowState[]
}

export const workflowStatesService = {
  async list(): Promise<WorkflowState[]> {
    const { data } = await apiClient.get<ListResponse>('/workflow-states')
    return data.states
  },
}
