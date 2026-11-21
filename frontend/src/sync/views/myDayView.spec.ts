import { describe, expect, it } from 'vitest'
import {
  BUILTIN_VIEWS,
  MY_DAY_VIEW,
  MY_OPEN_VIEW,
  findBuiltinView,
} from '@nosdesk/core/sync/views/builtinViews'
import { buildPredicate } from '@nosdesk/core/sync/views/filter'
import type { CardData } from '@nosdesk/core/sync/views/types'
import type { WorkflowStateCategory } from '@nosdesk/core/types/workflow'
import { sortCards } from '@/composables/useTicketsSort'

const ME = 'agent-uuid'

function card(
  id: number,
  category: WorkflowStateCategory,
  due_date: string | null,
  assignee_uuid: string | null = ME,
): CardData {
  return {
    id,
    number: id,
    title: `Ticket ${id}`,
    workflow_state: { id, name: category, category, color: 'gray' },
    priority: 'none',
    assignee_uuid,
    due_date,
    created_at: `2026-11-0${id}T00:00:00Z`,
    updated_at: '2026-11-01T00:00:00Z',
    last_activity_at: '2026-11-01T00:00:00Z',
  }
}

function myDay(cards: CardData[]): number[] {
  const keep = buildPredicate(MY_DAY_VIEW.filter, { currentUserUuid: ME })
  const sort = MY_DAY_VIEW.shape.sort[0]
  return sortCards(cards.filter(keep), sort.field, sort.dir).map((c) => c.id)
}

describe('the My day view', () => {
  it('lists my open tickets by due day, the undated last', () => {
    const cards = [
      card(1, 'active', null),
      card(2, 'backlog', '2026-11-25T00:00:00Z'),
      card(3, 'triage', '2026-11-21T00:00:00+11:00'),
      card(4, 'in_review', '2026-11-20T00:00:00'),
    ]
    expect(myDay(cards)).toEqual([4, 3, 2, 1])
  })

  it("leaves out other people's tickets, unassigned ones, and closed ones", () => {
    const cards = [
      card(1, 'active', '2026-11-21T00:00:00', 'someone-else'),
      card(2, 'active', '2026-11-21T00:00:00', null),
      card(3, 'done', '2026-11-21T00:00:00'),
      card(4, 'cancelled', null),
      card(5, 'merged', null),
      card(6, 'active', '2026-11-22T00:00:00'),
    ]
    expect(myDay(cards)).toEqual([6])
  })

  it('groups on the due date and shows the Due column in place of Updated', () => {
    expect(MY_DAY_VIEW.shape.group_by.primary).toBe('due_date')
    const fields = MY_DAY_VIEW.shape.visible_card_fields
    expect(fields).toContain('due_date')
    expect(fields).not.toContain('last_activity_at')
  })

  it('is second in the view list, after My Open, and resolves from the URL', () => {
    expect(BUILTIN_VIEWS.slice(0, 2).map((v) => v.id)).toEqual([MY_OPEN_VIEW.id, 'my-day'])
    expect(findBuiltinView('my-day')).toBe(MY_DAY_VIEW)
    expect(findBuiltinView('due-soon')).toBeNull()
  })

  it('takes the place of My Active, whose links open My day', () => {
    expect(BUILTIN_VIEWS.map((v) => v.id)).not.toContain('my-active')
    expect(findBuiltinView('my-active')).toBe(MY_DAY_VIEW)
  })
})
