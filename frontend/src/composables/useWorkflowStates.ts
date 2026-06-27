import { computed, type ComputedRef } from 'vue'
import { useWorkflowStatesStore } from '@nosdesk/core/stores/workflowStates'
import type { WorkflowState, WorkflowStateCategory } from '@nosdesk/core/types/workflow'

/**
 * Reactive workflow state lookup by id. Returns `undefined` while the
 * store is still loading or if the id is unknown.
 */
export function useWorkflowState(
  id: ComputedRef<number | null | undefined> | (() => number | null | undefined),
): ComputedRef<WorkflowState | undefined> {
  const store = useWorkflowStatesStore()
  const getId = typeof id === 'function' ? id : () => id.value
  return computed(() => {
    const v = getId()
    return v == null ? undefined : store.findById(v)
  })
}

/**
 * Reactive list of states inside a category, ordered by position.
 * Useful for kanban columns and category-grouped pickers.
 */
export function useWorkflowStatesByCategory(
  category: WorkflowStateCategory,
): ComputedRef<WorkflowState[]> {
  const store = useWorkflowStatesStore()
  return computed(() => store.byCategory[category] ?? [])
}
