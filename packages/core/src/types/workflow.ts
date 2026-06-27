/**
 * Workflow state and category types.
 *
 * Categories are fixed at the system level (seven values, never extended
 * by plugins or admins). State names within each category are
 * workspace-configurable. Downstream UI reasons in categories: SLA
 * timers, dashboard rollups, kanban columns, and renders state names
 * for the user-facing labels.
 *
 * `merged` is the terminal category the backend assigns when a ticket
 * is consumed by a merge action. It is not user-pickable (the merge
 * action sets it programmatically), so `WORKFLOW_CATEGORIES` (driving
 * status dropdowns / kanban columns) excludes it while the type and
 * the `byCategory` store getter must still account for it.
 */
import { translate } from '../i18n'

export type WorkflowStateCategory =
  | 'triage'
  | 'backlog'
  | 'active'
  | 'in_review'
  | 'done'
  | 'cancelled'
  | 'merged'

/** Categories the user can pick from in dropdowns / sees as kanban
 *  columns. Excludes `merged`, which the backend sets via the merge
 *  action and is hidden from regular ticket lists. */
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
  'merged',
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
  /**
   * Per-state SLA pause flag (admin-editable). When true, the SLA
   * matcher stops the clock while a ticket sits in this state. Lets a
   * "Waiting on customer" status pause time even if it lives under
   * the active category.
   */
  pauses_sla: boolean
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
  merged: 'workflow-category-merged',
}

const CATEGORY_LABEL_FALLBACKS: Record<WorkflowStateCategory, string> = {
  triage: 'Triage',
  backlog: 'Backlog',
  active: 'Active',
  in_review: 'In Review',
  done: 'Done',
  cancelled: 'Cancelled',
  merged: 'Merged',
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

export type StatusBucket = 'open' | 'in-progress' | 'closed'
/** Collapse a workflow-state category into the coarse 3-colour visual
 *  bucket used by tiny status dots and the public guest badge. This is
 *  a presentation concern, NOT the old wire field. */
export function coarseStatusBucket(category: WorkflowStateCategory): StatusBucket {
  if (category === 'triage' || category === 'backlog') return 'open'
  if (category === 'active' || category === 'in_review') return 'in-progress'
  return 'closed'
}

/** Flat option row for status pickers (`CustomDropdown`, bulk bar,
 *  preview pane). Category headers use `disabled: true`; selectable
 *  state rows carry `category` + `color` so `WorkflowStateGlyph` can
 *  encode meaning as shape + hue (colour-blind friendly). */
export interface WorkflowDropdownOption {
  value: string
  label: string
  disabled?: boolean
  color?: string
  category?: WorkflowStateCategory
}

/** Build grouped workflow-state options for ticket status pickers.
 *  Returns an empty list until the store has loaded. */
export function buildWorkflowDropdownOptions(
  byCategory: Record<WorkflowStateCategory, WorkflowState[] | undefined>,
  loaded: boolean,
  statesCount: number,
): WorkflowDropdownOption[] {
  if (!loaded || statesCount === 0) return []
  const out: WorkflowDropdownOption[] = []
  for (const cat of WORKFLOW_CATEGORIES) {
    const states = byCategory[cat]
    if (!states || states.length === 0) continue
    out.push({ value: categoryHeaderValue(cat), label: getCategoryLabel(cat), disabled: true })
    for (const s of states) {
      out.push({
        value: String(s.id),
        label: s.name,
        color: s.color,
        category: s.category,
      })
    }
  }
  return out
}
