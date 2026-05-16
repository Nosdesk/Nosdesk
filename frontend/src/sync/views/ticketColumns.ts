/**
 * Ticket-list column registry. Single source of truth for the
 * tickets table — every column the user can switch on lives
 * here, declared once. The view component reads the registry
 * to render headers + cells; the column-picker menu reads it
 * to build its checklist.
 *
 * Per the NN/g data-tables guide we ship a focused default
 * set (around six columns) and hide the rest behind the
 * picker. The user controls visibility, order, and width per
 * view; choices persist to localStorage and, when the view is
 * editable, can be promoted to the saved view's `shape.columns`.
 */
import type { CardData } from './types'

export type ColumnId =
  | 'id'
  | 'title'
  | 'workflow_state'
  | 'priority'
  | 'assignee'
  | 'requester'
  | 'category'
  | 'cycle'
  | 'due_date'
  | 'last_activity'
  | 'created_at'
  | 'sla'
  | 'kb_gap'
  | 'devices'
  | 'recurrence'

export interface ListColumn {
  /** Stable id used in storage and shape.columns. */
  id: ColumnId
  /** Header label, English fallback. Short, table headers are not
   * the place for prose. Consumers should prefer `labelKey` via
   * `translate(col.labelKey, undefined, col.label)`. */
  label: string
  /** Fluent key for the header label. Resolved at render time so
   * the active locale wins. */
  labelKey: string
  /** Default pixel width when the user hasn't dragged a resize
   * handle and the saved view doesn't carry its own setting. */
  defaultWidthPx: number
  /** Resize lower bound, narrower than this the column is illegible. */
  minWidthPx: number
  /** Resize upper bound, wider than this wastes screen real estate. */
  maxWidthPx: number
  /** When true the column flexes to fill remaining space rather
   * than rendering at its own fixed width. The renderer still
   * applies a max-width so very wide displays don't blow up the
   * line length. Today only the title flexes. */
  flex?: boolean
  /** Default visibility, tuned to fit a typical agent workflow:
   *  id, title, status, priority, assignee, last_activity. The
   *  rest are opt-in. */
  defaultVisible: boolean
  /** Whether the user can sort by this column. */
  sortKey: string | null
  /** Cell text alignment. */
  align: 'left' | 'right' | 'center'
  /** Surfaced in the picker menu as the column's tooltip. English
   * fallback. Consumers should prefer `descriptionKey`. */
  description: string
  /** Fluent key for the picker tooltip. */
  descriptionKey: string
}

/**
 * The set of fields on `CardData` whose change should re-render a
 * row. Used as the v-memo key inside the table so SSE updates
 * land surgically: changing one ticket's status mutates one row
 * rather than the entire visible list.
 */
export function rowMemoKey(card: CardData): unknown[] {
  return [
    card.id,
    card.title,
    card.workflow_state.id,
    card.priority,
    card.assignee_uuid,
    card.requester_uuid,
    card.category_id,
    card.cycle_id,
    card.due_date,
    card.last_activity_at,
    card.kb_gap_signal,
    card.sla?.target_at,
    card.sla?.breached,
    card.sla?.paused,
    card.affected_devices?.count,
    card.recurrence_rule,
  ]
}

export const TICKET_COLUMNS: readonly ListColumn[] = [
  {
    id: 'id',
    label: '#',
    labelKey: 'tickets-column-id',
    defaultWidthPx: 64,
    minWidthPx: 48,
    maxWidthPx: 120,
    defaultVisible: true,
    sortKey: 'id',
    align: 'left',
    description: 'Ticket number',
    descriptionKey: 'tickets-column-id-description',
  },
  {
    id: 'title',
    label: 'Title',
    labelKey: 'tickets-column-title',
    defaultWidthPx: 400,
    minWidthPx: 160,
    maxWidthPx: 720,
    flex: true,
    defaultVisible: true,
    sortKey: 'title',
    align: 'left',
    description: 'Ticket subject',
    descriptionKey: 'tickets-column-title-description',
  },
  {
    id: 'workflow_state',
    label: 'Status',
    labelKey: 'tickets-column-status',
    defaultWidthPx: 140,
    minWidthPx: 90,
    maxWidthPx: 240,
    // Default-hidden because the always-present 24px state-cell
    // (rendered by every TicketRow as a leading colour-coded dot)
    // already carries the workflow-state signal at-a-glance. Users
    // who want the explicit text label can re-enable via the
    // column-visibility menu. Linear's "icon-only" mode default,
    // text label is opt-in.
    defaultVisible: false,
    sortKey: 'workflow_state.name',
    align: 'left',
    description: 'Workflow state',
    descriptionKey: 'tickets-column-status-description',
  },
  {
    id: 'priority',
    label: 'Priority',
    labelKey: 'tickets-column-priority',
    defaultWidthPx: 88,
    minWidthPx: 64,
    maxWidthPx: 160,
    defaultVisible: true,
    sortKey: 'priority',
    align: 'left',
    description: 'Priority',
    descriptionKey: 'tickets-column-priority-description',
  },
  {
    id: 'assignee',
    label: 'Assignee',
    labelKey: 'tickets-column-assignee',
    defaultWidthPx: 140,
    minWidthPx: 90,
    maxWidthPx: 240,
    defaultVisible: true,
    sortKey: null,
    align: 'left',
    description: 'Who owns the ticket',
    descriptionKey: 'tickets-column-assignee-description',
  },
  {
    id: 'requester',
    label: 'Requester',
    labelKey: 'tickets-column-requester',
    defaultWidthPx: 140,
    minWidthPx: 90,
    maxWidthPx: 240,
    defaultVisible: false,
    sortKey: null,
    align: 'left',
    description: 'Who reported the ticket',
    descriptionKey: 'tickets-column-requester-description',
  },
  {
    id: 'category',
    label: 'Category',
    labelKey: 'tickets-column-category',
    defaultWidthPx: 120,
    minWidthPx: 80,
    maxWidthPx: 240,
    defaultVisible: false,
    sortKey: 'category_id',
    align: 'left',
    description: 'Ticket category tag',
    descriptionKey: 'tickets-column-category-description',
  },
  {
    id: 'cycle',
    label: 'Cycle',
    labelKey: 'tickets-column-cycle',
    defaultWidthPx: 110,
    minWidthPx: 80,
    maxWidthPx: 200,
    defaultVisible: false,
    sortKey: 'cycle_id',
    align: 'left',
    description: 'Cycle membership',
    descriptionKey: 'tickets-column-cycle-description',
  },
  {
    id: 'due_date',
    label: 'Due',
    labelKey: 'tickets-column-due-date',
    defaultWidthPx: 96,
    minWidthPx: 72,
    maxWidthPx: 160,
    defaultVisible: false,
    sortKey: 'due_date',
    align: 'left',
    description: 'Calendar deadline',
    descriptionKey: 'tickets-column-due-date-description',
  },
  {
    id: 'last_activity',
    label: 'Updated',
    labelKey: 'tickets-column-last-activity',
    defaultWidthPx: 96,
    minWidthPx: 72,
    maxWidthPx: 160,
    defaultVisible: true,
    sortKey: 'last_activity_at',
    align: 'left',
    description: 'When the ticket last changed',
    descriptionKey: 'tickets-column-last-activity-description',
  },
  {
    id: 'created_at',
    label: 'Created',
    labelKey: 'tickets-column-created-at',
    defaultWidthPx: 96,
    minWidthPx: 72,
    maxWidthPx: 160,
    defaultVisible: false,
    sortKey: 'created_at',
    align: 'left',
    description: 'When the ticket was opened',
    descriptionKey: 'tickets-column-created-at-description',
  },
  {
    id: 'sla',
    label: 'SLA',
    labelKey: 'tickets-column-sla',
    defaultWidthPx: 88,
    minWidthPx: 64,
    maxWidthPx: 160,
    defaultVisible: false,
    sortKey: null,
    align: 'center',
    description: 'SLA pill (green / amber / red)',
    descriptionKey: 'tickets-column-sla-description',
  },
  {
    id: 'kb_gap',
    label: 'KB',
    labelKey: 'tickets-column-kb-gap',
    defaultWidthPx: 56,
    minWidthPx: 48,
    maxWidthPx: 120,
    defaultVisible: false,
    sortKey: null,
    align: 'center',
    description: 'Knowledge-gap signal',
    descriptionKey: 'tickets-column-kb-gap-description',
  },
  {
    id: 'devices',
    label: 'Devices',
    labelKey: 'tickets-column-devices',
    defaultWidthPx: 80,
    minWidthPx: 56,
    maxWidthPx: 160,
    defaultVisible: false,
    sortKey: null,
    align: 'center',
    description: 'Affected device count',
    descriptionKey: 'tickets-column-devices-description',
  },
  {
    id: 'recurrence',
    label: 'Recur',
    labelKey: 'tickets-column-recurrence',
    defaultWidthPx: 64,
    minWidthPx: 48,
    maxWidthPx: 120,
    defaultVisible: false,
    sortKey: null,
    align: 'center',
    description: 'Recurring ticket marker',
    descriptionKey: 'tickets-column-recurrence-description',
  },
] as const

export const DEFAULT_VISIBLE_COLUMNS: readonly ColumnId[] = TICKET_COLUMNS
  .filter((c) => c.defaultVisible)
  .map((c) => c.id)

export function columnById(id: ColumnId): ListColumn | undefined {
  return TICKET_COLUMNS.find((c) => c.id === id)
}
