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
  /** Header label. Short — table headers are not the place for prose. */
  label: string
  /** Default pixel width when the user hasn't dragged a resize
   * handle and the saved view doesn't carry its own setting. */
  defaultWidthPx: number
  /** Resize lower bound — narrower than this the column is illegible. */
  minWidthPx: number
  /** Resize upper bound — wider than this wastes screen real estate. */
  maxWidthPx: number
  /** When true the column flexes to fill remaining space rather
   * than rendering at its own fixed width. The renderer still
   * applies a max-width so very wide displays don't blow up the
   * line length. Today only the title flexes. */
  flex?: boolean
  /** Default visibility — tuned to fit a typical agent workflow:
   *  id, title, status, priority, assignee, last_activity. The
   *  rest are opt-in. */
  defaultVisible: boolean
  /** Whether the user can sort by this column. */
  sortKey: string | null
  /** Cell text alignment. */
  align: 'left' | 'right' | 'center'
  /** Surfaced in the picker menu as the column's tooltip. */
  description: string
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
    defaultWidthPx: 64,
    minWidthPx: 48,
    maxWidthPx: 120,
    defaultVisible: true,
    sortKey: 'id',
    align: 'left',
    description: 'Ticket number',
  },
  {
    id: 'title',
    label: 'Title',
    defaultWidthPx: 400,
    minWidthPx: 160,
    maxWidthPx: 720,
    flex: true,
    defaultVisible: true,
    sortKey: 'title',
    align: 'left',
    description: 'Ticket subject',
  },
  {
    id: 'workflow_state',
    label: 'Status',
    defaultWidthPx: 140,
    minWidthPx: 90,
    maxWidthPx: 240,
    defaultVisible: true,
    sortKey: 'workflow_state.name',
    align: 'left',
    description: 'Workflow state',
  },
  {
    id: 'priority',
    label: 'Priority',
    defaultWidthPx: 88,
    minWidthPx: 64,
    maxWidthPx: 160,
    defaultVisible: true,
    sortKey: 'priority',
    align: 'left',
    description: 'Priority',
  },
  {
    id: 'assignee',
    label: 'Assignee',
    defaultWidthPx: 140,
    minWidthPx: 90,
    maxWidthPx: 240,
    defaultVisible: true,
    sortKey: null,
    align: 'left',
    description: 'Who owns the ticket',
  },
  {
    id: 'requester',
    label: 'Requester',
    defaultWidthPx: 140,
    minWidthPx: 90,
    maxWidthPx: 240,
    defaultVisible: false,
    sortKey: null,
    align: 'left',
    description: 'Who reported the ticket',
  },
  {
    id: 'category',
    label: 'Category',
    defaultWidthPx: 120,
    minWidthPx: 80,
    maxWidthPx: 240,
    defaultVisible: false,
    sortKey: 'category_id',
    align: 'left',
    description: 'Ticket category tag',
  },
  {
    id: 'cycle',
    label: 'Cycle',
    defaultWidthPx: 110,
    minWidthPx: 80,
    maxWidthPx: 200,
    defaultVisible: false,
    sortKey: 'cycle_id',
    align: 'left',
    description: 'Cycle membership',
  },
  {
    id: 'due_date',
    label: 'Due',
    defaultWidthPx: 96,
    minWidthPx: 72,
    maxWidthPx: 160,
    defaultVisible: false,
    sortKey: 'due_date',
    align: 'left',
    description: 'Calendar deadline',
  },
  {
    id: 'last_activity',
    label: 'Updated',
    defaultWidthPx: 96,
    minWidthPx: 72,
    maxWidthPx: 160,
    defaultVisible: true,
    sortKey: 'last_activity_at',
    align: 'left',
    description: 'When the ticket last changed',
  },
  {
    id: 'created_at',
    label: 'Created',
    defaultWidthPx: 96,
    minWidthPx: 72,
    maxWidthPx: 160,
    defaultVisible: false,
    sortKey: 'created_at',
    align: 'left',
    description: 'When the ticket was opened',
  },
  {
    id: 'sla',
    label: 'SLA',
    defaultWidthPx: 88,
    minWidthPx: 64,
    maxWidthPx: 160,
    defaultVisible: false,
    sortKey: null,
    align: 'center',
    description: 'SLA pill (green / amber / red)',
  },
  {
    id: 'kb_gap',
    label: 'KB',
    defaultWidthPx: 56,
    minWidthPx: 48,
    maxWidthPx: 120,
    defaultVisible: false,
    sortKey: null,
    align: 'center',
    description: 'Knowledge-gap signal',
  },
  {
    id: 'devices',
    label: 'Devices',
    defaultWidthPx: 80,
    minWidthPx: 56,
    maxWidthPx: 160,
    defaultVisible: false,
    sortKey: null,
    align: 'center',
    description: 'Affected device count',
  },
  {
    id: 'recurrence',
    label: 'Recur',
    defaultWidthPx: 64,
    minWidthPx: 48,
    maxWidthPx: 120,
    defaultVisible: false,
    sortKey: null,
    align: 'center',
    description: 'Recurring ticket marker',
  },
] as const

export const DEFAULT_VISIBLE_COLUMNS: readonly ColumnId[] = TICKET_COLUMNS
  .filter((c) => c.defaultVisible)
  .map((c) => c.id)

export function columnById(id: ColumnId): ListColumn | undefined {
  return TICKET_COLUMNS.find((c) => c.id === id)
}
