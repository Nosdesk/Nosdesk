/**
 * Ticket-list column registry. Single source of truth for the
 * tickets table — every column the user can switch on lives
 * here, declared once. The view component reads the registry
 * to render headers + cells; the column-picker menu reads it
 * to build its checklist.
 *
 * Per the NN/g data-tables guide we ship a focused default
 * set (around six columns) and hide the rest behind the
 * picker. The user controls visibility per view; the choice
 * persists to localStorage and, when the view is editable,
 * can be promoted to the saved view's `shape.visible_card_fields`.
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
  /** Stable id used in storage and shape.visible_card_fields. */
  id: ColumnId
  /** Header label. Short — table headers are not the place for prose. */
  label: string
  /** Tailwind width class. Empty string = column flexes (only the
   *  title column flexes today). */
  width: string
  /** Default visibility — tuned to fit a typical agent workflow:
   *  id, title, status, priority, assignee, last_activity. The
   *  rest are opt-in. */
  defaultVisible: boolean
  /** Whether the user can sort by this column (drives header
   *  click affordance). The sort key is the dot-path read from
   *  CardData; helps the renderer stay generic. */
  sortKey: string | null
  /** Tailwind text-align utility. Most cells flow left; numerics
   *  and icons centre. */
  align: 'left' | 'right' | 'center'
  /** Short tooltip explaining what the column shows. Surfaced in
   *  the picker menu, not the table header. */
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
    width: 'w-16',
    defaultVisible: true,
    sortKey: 'id',
    align: 'left',
    description: 'Ticket number',
  },
  {
    id: 'title',
    label: 'Title',
    width: '',
    defaultVisible: true,
    sortKey: 'title',
    align: 'left',
    description: 'Ticket subject',
  },
  {
    id: 'workflow_state',
    label: 'Status',
    width: 'w-32',
    defaultVisible: true,
    sortKey: 'workflow_state.name',
    align: 'left',
    description: 'Workflow state',
  },
  {
    id: 'priority',
    label: 'Priority',
    width: 'w-20',
    defaultVisible: true,
    sortKey: 'priority',
    align: 'left',
    description: 'Priority',
  },
  {
    id: 'assignee',
    label: 'Assignee',
    width: 'w-32',
    defaultVisible: true,
    sortKey: null,
    align: 'left',
    description: 'Who owns the ticket',
  },
  {
    id: 'requester',
    label: 'Requester',
    width: 'w-32',
    defaultVisible: false,
    sortKey: null,
    align: 'left',
    description: 'Who reported the ticket',
  },
  {
    id: 'category',
    label: 'Category',
    width: 'w-28',
    defaultVisible: false,
    sortKey: 'category_id',
    align: 'left',
    description: 'Ticket category tag',
  },
  {
    id: 'cycle',
    label: 'Cycle',
    width: 'w-24',
    defaultVisible: false,
    sortKey: 'cycle_id',
    align: 'left',
    description: 'Cycle membership',
  },
  {
    id: 'due_date',
    label: 'Due',
    width: 'w-20',
    defaultVisible: false,
    sortKey: 'due_date',
    align: 'left',
    description: 'Calendar deadline',
  },
  {
    id: 'last_activity',
    label: 'Updated',
    width: 'w-20',
    defaultVisible: true,
    sortKey: 'last_activity_at',
    align: 'left',
    description: 'When the ticket last changed',
  },
  {
    id: 'created_at',
    label: 'Created',
    width: 'w-20',
    defaultVisible: false,
    sortKey: 'created_at',
    align: 'left',
    description: 'When the ticket was opened',
  },
  {
    id: 'sla',
    label: 'SLA',
    width: 'w-16',
    defaultVisible: false,
    sortKey: null,
    align: 'center',
    description: 'SLA pill (green / amber / red)',
  },
  {
    id: 'kb_gap',
    label: 'KB',
    width: 'w-12',
    defaultVisible: false,
    sortKey: null,
    align: 'center',
    description: 'Knowledge-gap signal',
  },
  {
    id: 'devices',
    label: 'Devices',
    width: 'w-16',
    defaultVisible: false,
    sortKey: null,
    align: 'center',
    description: 'Affected device count',
  },
  {
    id: 'recurrence',
    label: 'Recur',
    width: 'w-12',
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
