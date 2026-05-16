/**
 * Workflow state and category types.
 *
 * Categories are fixed at the system level (six values, never extended
 * by plugins or admins). State names within each category are
 * workspace-configurable. Downstream UI reasons in categories — SLA
 * timers, dashboard rollups, kanban columns — and renders state names
 * for the user-facing labels.
 */
import { translate } from '@/i18n'

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

/**
 * Fluent keys for each workflow category. Wrapped by `getCategoryLabel`
 * for the common "give me a localized string" path; exposed directly so
 * components that already have a Fluent context (e.g. `useFluent().$t`)
 * can resolve without going through the module-level translate helper.
 */
export const CATEGORY_LABEL_KEYS: Record<WorkflowStateCategory, string> = {
  triage: 'workflow-category-triage',
  backlog: 'workflow-category-backlog',
  active: 'workflow-category-active',
  in_review: 'workflow-category-in-review',
  done: 'workflow-category-done',
  cancelled: 'workflow-category-cancelled',
}

const CATEGORY_LABEL_FALLBACKS: Record<WorkflowStateCategory, string> = {
  triage: 'Triage',
  backlog: 'Backlog',
  active: 'Active',
  in_review: 'In Review',
  done: 'Done',
  cancelled: 'Cancelled',
}

/**
 * Localized display label for a workflow category. Resolves through
 * the module-level `translate()` helper so it works outside Vue
 * setup contexts (e.g. pure TS callers). Falls back to the English
 * canonical name if the Fluent bundle hasn't initialised yet, which
 * keeps tests and bootstrap call sites from rendering raw keys.
 */
export function getCategoryLabel(category: WorkflowStateCategory): string {
  return translate(
    CATEGORY_LABEL_KEYS[category],
    undefined,
    CATEGORY_LABEL_FALLBACKS[category],
  )
}

/**
 * Sentinel prefix for non-selectable dropdown rows that render a
 * category-group header inside a flat option list. Both the option
 * builder (TicketDetails) and the option handler (TicketDetails +
 * CustomDropdown) check for this prefix; keeping the literal in one
 * place stops a typo on either side from silently breaking
 * click-to-select.
 */
export const CATEGORY_HEADER_VALUE_PREFIX = '__cat_'

export function categoryHeaderValue(category: WorkflowStateCategory): string {
  return `${CATEGORY_HEADER_VALUE_PREFIX}${category}`
}

export function isCategoryHeaderValue(value: string): boolean {
  return value.startsWith(CATEGORY_HEADER_VALUE_PREFIX)
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
