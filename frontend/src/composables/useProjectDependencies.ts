/**
 * Project dependency edges (blocks / blocked_by relations) via
 * Pinia Colada: cache-first with silent revalidate, replacing the
 * old fetch-into-a-ref-and-swallow-errors pattern. Arrows are
 * supplementary, so consumers render the board regardless and
 * surface a slim retry notice on failure.
 */
import { computed, toValue, type MaybeRefOrGetter } from 'vue'
import { useQuery } from '@pinia/colada'
import {
  dependenciesService,
  type DependencyEdge,
} from '@nosdesk/core/services/dependenciesService'

export function useProjectDependencies(projectId: MaybeRefOrGetter<number>) {
  const query = useQuery({
    key: () => ['project-dependencies', toValue(projectId)],
    query: () => dependenciesService.forProject(toValue(projectId)),
  })

  const edges = computed<DependencyEdge[]>(() => query.data.value ?? [])
  const failed = computed(
    () => query.status.value === 'error' && edges.value.length === 0,
  )

  return { edges, failed, refetch: query.refetch }
}
