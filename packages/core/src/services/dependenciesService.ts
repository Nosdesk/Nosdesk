import apiClient from '../apiClient'

export type RelationType = 'blocks' | 'blocked_by' | 'related' | 'duplicate_of'

export interface DependencyEdge {
  from: number
  to: number
  relation_type: RelationType
}

export const dependenciesService = {
  /** Returns linked_tickets entries where both ends fall inside the
   * project. Today the GanttBoard renders `blocks`-typed edges as
   * arrows; other relation kinds round-trip so a later filter can
   * surface them without a service-layer change. */
  async forProject(projectId: number): Promise<DependencyEdge[]> {
    const { data } = await apiClient.get<DependencyEdge[]>(
      `/projects/${projectId}/dependencies`,
    )
    return data
  },
}
