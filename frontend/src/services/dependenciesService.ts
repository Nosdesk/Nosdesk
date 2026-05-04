import apiClient from './apiConfig'

export interface DependencyEdge {
  from: number
  to: number
  link_type: string
}

export const dependenciesService = {
  /** Returns linked_tickets entries where both ends fall inside the
   * project. Today the GanttBoard renders `blocks`-typed edges as
   * arrows; other link kinds round-trip so a later filter can
   * surface them without a service-layer change. */
  async forProject(projectId: number): Promise<DependencyEdge[]> {
    const { data } = await apiClient.get<DependencyEdge[]>(
      `/projects/${projectId}/dependencies`,
    )
    return data
  },
}
