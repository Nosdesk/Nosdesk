/**
 * Workflow state and category types.
 *
 * Categories are fixed at the system level (six values, never extended
 * by plugins or admins). State names within each category are
 * workspace-configurable. Downstream UI reasons in categories — SLA
 * timers, dashboard rollups, kanban columns — and renders state names
 * for the user-facing labels.
 */

export type WorkflowStateCategory =
  | 'triage'
  | 'backlog'
  | 'active'
  | 'in_review'
  | 'done'
  | 'cancelled'

export const WORKFLOW_CATEGORIES: WorkflowStateCategory[] = [
  'triage',
  'backlog',
  'active',
  'in_review',
  'done',
  'cancelled',
]

/** Categories that don't transition further on their own. */
export const TERMINAL_CATEGORIES: ReadonlySet<WorkflowStateCategory> = new Set([
  'done',
  'cancelled',
])

export interface WorkflowState {
  id: number
  name: string
  category: WorkflowStateCategory
  color: string
  position: number
  is_default: boolean
  archived_at: string | null
  created_at: string
  created_by: string | null
}

export const CATEGORY_LABELS: Record<WorkflowStateCategory, string> = {
  triage: 'Triage',
  backlog: 'Backlog',
  active: 'Active',
  in_review: 'In Review',
  done: 'Done',
  cancelled: 'Cancelled',
}

/**
 * Folds the six-category model down to the legacy three-bucket status
 * string used by older parts of the UI. Triage and Backlog are "open";
 * Active and In Review are "in-progress"; Done and Cancelled are
 * "closed". Mirrors `WorkflowStateCategory::legacy_status()` on the
 * backend.
 */
export function legacyStatusFor(category: WorkflowStateCategory): 'open' | 'in-progress' | 'closed' {
  if (category === 'triage' || category === 'backlog') return 'open'
  if (category === 'active' || category === 'in_review') return 'in-progress'
  return 'closed'
}
