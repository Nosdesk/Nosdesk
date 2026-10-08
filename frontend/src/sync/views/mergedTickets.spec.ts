import { describe, expect, it } from 'vitest'
import {
  ALL_ACTIVE_VIEW,
  ALL_TICKETS_VIEW,
  MY_ACTIVE_VIEW,
  OVERDUE_VIEW,
  UNASSIGNED_VIEW,
} from '@nosdesk/core/sync/views/builtinViews'
import { buildPredicate } from '@nosdesk/core/sync/views/filter'
import type { CardData } from '@nosdesk/core/sync/views/types'
import {
  buildWorkflowDropdownOptions,
  type WorkflowState,
  type WorkflowStateCategory,
} from '@nosdesk/core/types/workflow'

function state(id: number, name: string, category: WorkflowStateCategory): WorkflowState {
  return {
    id,
    name,
    category,
    color: 'gray',
    position: id,
    is_default: false,
    archived_at: null,
    created_at: '2026-01-01T00:00:00Z',
    created_by: null,
    pauses_sla: false,
  }
}

const open = state(1, 'Open', 'active')
const done = state(2, 'Done', 'done')
const merged = state(3, 'Merged', 'merged')
const byCategory = {
  triage: [],
  backlog: [],
  active: [open],
  in_review: [],
  done: [done],
  cancelled: [],
  merged: [merged],
} as Record<WorkflowStateCategory, WorkflowState[]>

describe('a merged ticket', () => {
  it('shows its Merged state, and only that, in the status picker', () => {
    const options = buildWorkflowDropdownOptions(byCategory, true, 3, merged)
    expect(options.filter((o) => !o.disabled).map((o) => o.value)).toEqual(['3'])
    expect(options.find((o) => o.disabled)?.label).toBe('Merged')
  })

  it('leaves the picker as it was for any other ticket', () => {
    const options = buildWorkflowDropdownOptions(byCategory, true, 3, open)
    expect(options.filter((o) => !o.disabled).map((o) => o.value)).toEqual(['1', '2'])
  })

  it('is out of the active views, and in All Tickets', () => {
    const me = 'agent-uuid'
    const card = (s: WorkflowState): CardData => ({
      id: 7,
      number: 7,
      title: 'Printer on fire',
      workflow_state: { id: s.id, name: s.name, category: s.category, color: s.color },
      priority: 'none',
      assignee_uuid: null,
      due_date: '2020-01-01T00:00:00Z',
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
      last_activity_at: '2026-01-01T00:00:00Z',
    })
    const shows = (view: typeof ALL_ACTIVE_VIEW, c: CardData) =>
      buildPredicate(view.filter, { currentUserUuid: me })(c)

    const mine = { ...card(merged), assignee_uuid: me }
    expect(shows(ALL_ACTIVE_VIEW, card(merged))).toBe(false)
    expect(shows(UNASSIGNED_VIEW, card(merged))).toBe(false)
    expect(shows(OVERDUE_VIEW, card(merged))).toBe(false)
    expect(shows(MY_ACTIVE_VIEW, mine)).toBe(false)
    expect(shows(ALL_TICKETS_VIEW, card(merged))).toBe(true)

    // The same ticket while open is in each of them.
    expect(shows(ALL_ACTIVE_VIEW, card(open))).toBe(true)
    expect(shows(UNASSIGNED_VIEW, card(open))).toBe(true)
    expect(shows(OVERDUE_VIEW, card(open))).toBe(true)
    expect(shows(MY_ACTIVE_VIEW, { ...card(open), assignee_uuid: me })).toBe(true)
  })
})
